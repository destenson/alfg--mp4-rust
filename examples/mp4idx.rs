use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::Arc;
use mp4::{Mp4Box, Result, SidxBox};
use mp4::sidx::{sidx_to_seek_segments, SeekSegment};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: mp4idx <filename>");
        std::process::exit(1);
    }

    if let Err(err) = dump(&args[1]) {
        let _ = writeln!(io::stderr(), "{}", err);
    }
}


fn dump<P: AsRef<Path>>(filename: &P) -> mp4::Result<()> {
    let f = File::open(filename)?;
    let boxes = get_boxes(f)?;

    // print out boxes
    // for b in boxes.iter().take(0) {
    //     match b.name.as_str() {
    //         // "ftyp" => println!("[{}] size={} major_brand={} compatible_brands={}",
    //         //                   b.name, b.size, b.summary, b.summary),
    //         // "moov" => println!("[{}] size={} {}", b.name, b.size, b.summary),
    //         // "mvhd" => println!("[{}] size={} timescale={} duration={}",
    //         //                   b.name, b.size, b.summary, b.summary),
    //         // "trak" => println!("[{}] size={} {}", b.name, b.size, b.summary),
    //         // "mdia" => println!("[{}] size={} {}", b.name, b.size, b.summary),
    //         "tfhd" => {},
    //         "traf" => {},
    //         "trun" => {},
    //         "mfhd" => {}
    //         _ => println!("[{}] size={} {}", b.name, b.size, b.summary)
    //     }
    // }
    println!("[{}] size={} segments={}", "sidx", boxes.len(), boxes.iter().map(|b| b.segments().len()).sum::<usize>());
    
    for seg in boxes.iter().flat_map(|b| b.segments().iter()) {
        println!(
            "[{}] time={} duration={} offset={} size={}",
            // seg.box_type(),
            "sidx",
            seg.time_seconds,
            seg.duration_seconds,
            seg.byte_offset,
            seg.byte_size
        );
    }

    Ok(())
}


#[derive(Debug, Clone, PartialEq, Default)]
pub struct Segments {
    segments: Vec<SeekSegment>,
}

impl Segments {
    pub fn new(segments: Vec<SeekSegment>) -> Self {
        Segments { segments }
    }
    // pub fn new<M: Mp4Box + std::fmt::Debug>(m: &M) -> Self {
    //     let segments = if let Some(sidx) = m. {
    //         sidx_to_seek_segments(sidx)
    //     } else {
    //         vec![]
    //     };
    //     Segments { segments }
    // }

    pub fn segments(&self) -> &[SeekSegment] {
        &self.segments
    }
}

fn get_boxes(file: File) -> Result<Vec<Segments>> {
    let size = file.metadata()?.len();
    let reader = BufReader::new(file);
    let mp4 = mp4::Mp4Reader::read_header(reader, size)?;

    // collect known boxes
    let mut boxes = vec![
        build_box(&mp4.ftyp),
        build_box(&mp4.moov),
        build_box(&mp4.moov.mvhd),
    ];

    if let Some(mvex) = &mp4.moov.mvex {
        boxes.push(build_box(mvex));
        if let Some(mehd) = &mvex.mehd {
            boxes.push(build_box(mehd));
        }
        boxes.push(build_box(&mvex.trex));
    }

    // trak.
    for track in mp4.tracks().values() {
        boxes.push(build_box(&track.trak));
        boxes.push(build_box(&track.trak.tkhd));
        if let Some(edts) = &track.trak.edts {
            boxes.push(build_box(edts));
            if let Some(elst) = &edts.elst {
                boxes.push(build_box(elst));
            }
        }

        // trak.mdia
        let mdia = &track.trak.mdia;
        boxes.push(build_box(mdia));
        boxes.push(build_box(&mdia.mdhd));
        boxes.push(build_box(&mdia.hdlr));
        boxes.push(build_box(&track.trak.mdia.minf));

        // trak.mdia.minf
        let minf = &track.trak.mdia.minf;
        if let Some(vmhd) = &minf.vmhd {
            boxes.push(build_box(vmhd));
        }
        if let Some(smhd) = &minf.smhd {
            boxes.push(build_box(smhd));
        }

        // trak.mdia.minf.stbl
        let stbl = &track.trak.mdia.minf.stbl;
        boxes.push(build_box(stbl));
        boxes.push(build_box(&stbl.stsd));
        if let Some(avc1) = &stbl.stsd.avc1 {
            boxes.push(build_box(avc1));
        }
        if let Some(hev1) = &stbl.stsd.hev1 {
            boxes.push(build_box(hev1));
        }
        if let Some(mp4a) = &stbl.stsd.mp4a {
            boxes.push(build_box(mp4a));
        }
        boxes.push(build_box(&stbl.stts));
        if let Some(ctts) = &stbl.ctts {
            boxes.push(build_box(ctts));
        }
        if let Some(stss) = &stbl.stss {
            boxes.push(build_box(stss));
        }
        boxes.push(build_box(&stbl.stsc));
        boxes.push(build_box(&stbl.stsz));
        if let Some(stco) = &stbl.stco {
            boxes.push(build_box(stco));
        }
        if let Some(co64) = &stbl.co64 {
            boxes.push(build_box(co64));
        }
    }

    // If fragmented, add moof boxes.
    for moof in mp4.moofs.iter() {
        boxes.push(build_box(moof));
        boxes.push(build_box(&moof.mfhd));
        for traf in moof.trafs.iter() {
            boxes.push(build_box(traf));
            boxes.push(build_box(&traf.tfhd));
            if let Some(trun) = &traf.trun {
                boxes.push(build_box(trun));
            }
        }
    }

    // if let Some(sidx) = &mp4.sidx {
    //     boxes.push(build_box(sidx));
    // }
    // let (sidx, sidx_box_offset) = mp4.sidx.first().unwrap();
    // let sidx_box_size = sidx.box_size();
    
    let segments = mp4.sidx.iter().map(|sidx| {
        let (sidx, sidx_box_offset) = sidx;
        let sidx_box_size = sidx.box_size();
        let segs = sidx_to_seek_segments(sidx, *sidx_box_offset as u64, sidx_box_size);
        Segments::new(segs)
    })
        .collect();

    Ok(segments)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Box {
    name: String,
    size: u64,
    summary: String,
    indent: u32,
}

fn build_box<M: Mp4Box + std::fmt::Debug>(m: &M) -> Box {
    Box {
        name: m.box_type().to_string(),
        size: m.box_size(),
        summary: m.summary().unwrap(),
        indent: 0,
    }
}

// fn build_box_<M: Mp4Box + std::fmt::Debug>(m: &M) -> Box {
//     // if m.box_type() != mp4::BoxType::SidxBox {
//     //     return SeekSegment::default();
//     // }
//     println!("sidx: {}", m.to_json().unwrap().to_string());
//     // Segment {
//     //     start_time: m.start_time(),
//     //     duration: m.duration(),
//     //     offset: m.offset(),
//     //     size: m.size(),
//     // }
//     // sidx_to_seek_segments(m)
//     // Segments::default()
//     Segments::new(m)
// }

// /// segment information useful for seeking
// #[derive(Debug, Clone, PartialEq, Default)]
// pub struct SeekSegment {
//     pub time_seconds: f64,       // Time in seconds from start
//     pub duration_seconds: f64,    // Duration in seconds
//     pub byte_offset: u64,         // Byte offset in the file
//     pub byte_size: u32,           // Size in bytes
// }

pub struct DashSegment {
    pub start_time_seconds: f64,     // Start time of the segment
    pub duration_seconds: f64,        // Duration of the segment
    pub byte_range_start: u64,        // Start byte offset
    pub byte_range_end: u64,          // End byte offset
    pub contains_sap: bool,           // Stream Access Point (keyframe) flag
    pub sap_type: u8,                 // Type of access point (0 is usually I-frame)
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
