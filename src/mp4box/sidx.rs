use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::mp4box::*;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SidxBox {
    pub version: u8,
    pub flags: u32,
    pub reference_id: u32,
    pub timescale: u32,
    pub earliest_presentation_time: u64,
    pub first_offset: u64,
    pub references: Vec<SidxReference>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SidxReference {
    pub reference_type: u8,
    pub referenced_size: u32,
    pub subsegment_duration: u32,
    pub starts_with_sap: bool,
    pub sap_type: u8,
    pub sap_delta_time: u32,
}

impl Mp4Box for SidxBox {
    fn box_type(&self) -> BoxType {
        BoxType::SidxBox
    }

    fn box_size(&self) -> u64 {
        let mut size = HEADER_SIZE + HEADER_EXT_SIZE;
        size += 12; // reference_id + timescale + reserved
        size += if self.version == 0 { 8 } else { 16 }; // earliest_presentation_time + first_offset
        size += 4; // reserved + reference_count (2 bytes each)
        
        // Each reference entry is 12 bytes
        size += (self.references.len() as u64) * 12;
        
        size
    }

    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self).unwrap())
    }

    fn summary(&self) -> Result<String> {
        let s = format!(
            "reference_id={} timescale={} earliest_presentation_time={} first_offset={} references={}",
            self.reference_id,
            self.timescale,
            self.earliest_presentation_time,
            self.first_offset,
            self.references.len()
        );
        Ok(s)
    }
    
    
}

impl<R: Read> ReadBox<&mut R> for SidxBox {
    fn read_box(reader: &mut R, size: u64) -> Result<Self> {
        let (version, flags) = read_box_header_ext(reader)?;
        
        let reference_id = reader.read_u32::<BigEndian>()?;
        let timescale = reader.read_u32::<BigEndian>()?;
        
        let (earliest_presentation_time, first_offset) = if version == 0 {
            (
                reader.read_u32::<BigEndian>()? as u64,
                reader.read_u32::<BigEndian>()? as u64,
            )
        } else {
            (
                reader.read_u64::<BigEndian>()?,
                reader.read_u64::<BigEndian>()?,
            )
        };
        
        let reserved = reader.read_u16::<BigEndian>()?;
        if reserved != 0 {
            // Optional: warning for non-zero reserved value
        }
        
        let reference_count = reader.read_u16::<BigEndian>()? as usize;
        let mut references = Vec::with_capacity(reference_count);
        
        for _ in 0..reference_count {
            let first_byte = reader.read_u32::<BigEndian>()?;
            let reference_type = ((first_byte >> 31) & 0x01) as u8;
            let referenced_size = first_byte & 0x7FFFFFFF; // Mask the top bit
            
            let subsegment_duration = reader.read_u32::<BigEndian>()?;
            
            let third_dword = reader.read_u32::<BigEndian>()?;
            let starts_with_sap = (third_dword >> 31) & 0x01 == 1;
            let sap_type = ((third_dword >> 28) & 0x07) as u8;
            let sap_delta_time = third_dword & 0x0FFFFFFF; // Mask the top 4 bits
            
            references.push(SidxReference {
                reference_type,
                referenced_size,
                subsegment_duration,
                starts_with_sap,
                sap_type,
                sap_delta_time,
            });
        }
        
        Ok(SidxBox {
            version,
            flags,
            reference_id,
            timescale,
            earliest_presentation_time,
            first_offset,
            references,
        })
    }
}

impl<T: Write> WriteBox<&mut T> for SidxBox {
    fn write_box(&self, writer: &mut T) -> Result<u64> {
        let size = self.box_size();
        BoxHeader::new(self.box_type(), size).write(writer)?;
        
        write_box_header_ext(writer, self.version, self.flags)?;
        
        writer.write_u32::<BigEndian>(self.reference_id)?;
        writer.write_u32::<BigEndian>(self.timescale)?;
        
        if self.version == 0 {
            writer.write_u32::<BigEndian>(self.earliest_presentation_time as u32)?;
            writer.write_u32::<BigEndian>(self.first_offset as u32)?;
        } else {
            writer.write_u64::<BigEndian>(self.earliest_presentation_time)?;
            writer.write_u64::<BigEndian>(self.first_offset)?;
        }
        
        writer.write_u16::<BigEndian>(0)?; // reserved
        writer.write_u16::<BigEndian>(self.references.len() as u16)?;
        
        for reference in &self.references {
            let first_byte = ((reference.reference_type as u32) << 31) | reference.referenced_size;
            writer.write_u32::<BigEndian>(first_byte)?;
            
            writer.write_u32::<BigEndian>(reference.subsegment_duration)?;
            
            let sap_bits = if reference.starts_with_sap {
                0x80000000 | ((reference.sap_type as u32) << 28) | reference.sap_delta_time
            } else {
                reference.sap_delta_time
            };
            writer.write_u32::<BigEndian>(sap_bits)?;
        }
        
        Ok(size)
    }
}

pub fn sidx_to_seek_segments(sidx: &SidxBox, sidx_box_offset: u64, sidx_box_size: u64) -> Vec<SeekSegment> {
    let mut segments = Vec::new();
    let timescale = sidx.timescale as f64;

    // Base offset is the position after the SIDX box + first_offset from SIDX
    let base_offset = sidx_box_offset + sidx_box_size + sidx.first_offset;

    let mut current_time = sidx.earliest_presentation_time as f64 / timescale;
    let mut current_offset = base_offset;

    for reference in &sidx.references {
        let duration_seconds = reference.subsegment_duration as f64 / timescale;

        segments.push(SeekSegment {
            time_seconds: current_time,
            duration_seconds,
            byte_offset: current_offset,
            byte_size: reference.referenced_size,
        });

        // Update for next segment
        current_time += duration_seconds;
        current_offset += reference.referenced_size as u64;
    }

    segments
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeekSegment {
    pub time_seconds: f64,       // Time in seconds from start
    pub duration_seconds: f64,   // Duration in seconds
    pub byte_offset: u64,        // Byte offset in the file
    pub byte_size: u32,          // Size in bytes
}

pub struct DashSegment {
    pub start_time_seconds: f64,     // Start time of the segment
    pub duration_seconds: f64,       // Duration of the segment
    pub byte_range_start: u64,       // Start byte offset
    pub byte_range_end: u64,         // End byte offset
    pub contains_sap: bool,          // Stream Access Point (keyframe) flag
    pub sap_type: u8,                // Type of access point (0 is usually I-frame)
}

pub fn parse_dash_sidx(
    sidx: &SidxBox,
    sidx_box_offset: u64,
    sidx_box_size: u64
) -> Vec<DashSegment> {
    let mut segments = Vec::new();
    let timescale = sidx.timescale as f64;

    // Base offset is the position after the SIDX box + first_offset from SIDX
    let base_offset = sidx_box_offset + sidx_box_size + sidx.first_offset;

    let mut current_time = sidx.earliest_presentation_time as f64 / timescale;
    let mut current_offset = base_offset;

    for reference in &sidx.references {
        let duration_seconds = reference.subsegment_duration as f64 / timescale;

        segments.push(DashSegment {
            start_time_seconds: current_time,
            duration_seconds,
            byte_range_start: current_offset,
            byte_range_end: current_offset + reference.referenced_size as u64 - 1,
            contains_sap: reference.starts_with_sap,
            sap_type: reference.sap_type,
        });

        // Update for next segment
        current_time += duration_seconds;
        current_offset += reference.referenced_size as u64;
    }

    segments
}

// Utility function to find the segment containing a specific time
pub fn find_segment_for_time(segments: &[DashSegment], time_seconds: f64) -> Option<&DashSegment> {
    segments.iter().find(|segment| {
        time_seconds >= segment.start_time_seconds &&
            time_seconds < segment.start_time_seconds + segment.duration_seconds
    })
}

// Generate a URL with byte range for a specific segment
pub fn get_range_request(base_url: &str, segment: &DashSegment) -> String {
    format!(
        "{}; Range: bytes={}-{}",
        base_url,
        segment.byte_range_start,
        segment.byte_range_end
    )
}
