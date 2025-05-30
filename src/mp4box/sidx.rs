use serde::{Deserialize, Serialize};
use std::fmt;
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