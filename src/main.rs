#[macro_use]
extern crate lazy_static;
extern crate byteorder;

use byteorder::{BigEndian, ByteOrder};
use std::collections::HashMap;
use std::env;
use std::fs;

const THRESHOLD: usize = 50 * (1 << 10);
lazy_static! {
    static ref MP3_BIT_RATES: HashMap<u32, u32> = [
        (0b0001, 32000),
        (0b0010, 40000),
        (0b0011, 48000),
        (0b0100, 56000),
        (0b0101, 64000),
        (0b0110, 80000),
        (0b0111, 96000),
        (0b1000, 112000),
        (0b1001, 128000),
        (0b1010, 160000),
        (0b1011, 192000),
        (0b1100, 224000),
        (0b1101, 256000),
        (0b1110, 320000),
    ]
    .iter()
    .copied()
    .collect();

    static ref MP3_SAMPLE_RATES: HashMap<u32, u32> = [(0b00, 44100), (0b01, 48000), (0b10, 3200),]
        .iter()
        .copied()
        .collect();
}

struct MyVec<'a> {
    arr: &'a Vec<u8>,
    idx: usize,
}

impl<'a> MyVec<'a> {
    fn new(arr: &'a Vec<u8>) -> MyVec {
        MyVec { arr, idx: 0 }
    }

    fn read(&mut self, i: usize) -> &'a [u8] {
        if self.idx + i >= self.arr.len() {
            return &[];
        }
        let tmp = self.idx;
        self.idx += i;
        &self.arr[tmp..self.idx]
    }

    fn len(&self) -> usize {
        self.arr.len() - self.idx
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    for file in &args[1..] {
        process_file(file);
    }
}

fn process_file(file: &String) {
    // deobfuscate files and extract mp3s
    let buffer: Vec<u8> = fs::read(file).unwrap();
    let mut extracted: Vec<(Vec<u8>, usize)> = Vec::new();
    for i in 0..4 {
        let mut tmp_buffer = buffer.clone();
        deobfs(&mut tmp_buffer, i);
        extracted.extend(extract_mp3(tmp_buffer));
    }

    // sort extracted mp3s by the order they appear in
    extracted.sort_by(|a, b| a.1.cmp(&b.1));

    // write mp3s to file
    for (i, mp3) in extracted.iter().enumerate() {
        let outfile = format!("{}.{}.mp3", file, i + 1);
        println!("writing {}", outfile);
        fs::write(outfile, &mp3.0[..]).unwrap();
    }
}

fn deobfs(buffer: &mut Vec<u8>, offset: usize) {
    for i in 0..buffer.len() - 1 {
        if i % 4 == offset {
            buffer.swap(i, i + 1);
        }
    }
}

fn extract_mp3(s: Vec<u8>) -> Vec<(Vec<u8>, usize)> {
    // extract all mp3s found in data stream
    // adapted from https://gist.github.com/RavuAlHemio/9376cf495c82be9c8778
    let mut stream = MyVec::new(&s);
    let total_stream_len: usize = stream.len();

    let mut header: Vec<u8> = vec![0];
    header.extend(stream.read(3));

    let mut mp3_stream: Vec<u8> = Vec::new();
    let mut is_mp3 = false;

    // return value
    let mut extracted_mp3s: Vec<(Vec<u8>, usize)> = Vec::new();

    loop {
        if !is_mp3 {
            if mp3_stream.len() > THRESHOLD {
                let offset: usize = total_stream_len - stream.len() - mp3_stream.len();
                extracted_mp3s.push((mp3_stream.clone(), offset));
            }
            mp3_stream.clear();
        }

        is_mp3 = false;

        // read header_num
        header.remove(0);
        let x = stream.read(1);
        if x.len() == 0 {
            break;
        }
        header.push(x[0]);
        let header_num = BigEndian::read_u32(&header[..]);

        // frame sync
        if header_num & 0xFFE00000 != 0xFFE00000 {
            continue;
        }

        // MPEG version
        let mpeg_version = (header_num & 0x00180000) >> 19;
        if mpeg_version == 0b01 || mpeg_version != 0b11 {
            continue;
        }

        // MPEG layer
        let mpeg_layer = (header_num & 0x00060000) >> 17;
        if mpeg_layer == 0b00 || mpeg_layer != 0b01 {
            continue;
        }

        // bitrate
        let bit_rate_idx = (header_num & 0x0000F000) >> 12;
        if bit_rate_idx == 0b0000 || bit_rate_idx == 0b1111 {
            continue;
        }
        let bit_rate = match MP3_BIT_RATES.get(&bit_rate_idx) {
            Some(val) => val,
            None => continue,
        };

        // sample rate
        let sample_rate_idx = (header_num & 0x00000C00) >> 10;
        if sample_rate_idx == 0b11 {
            continue;
        }
        let sample_rate = match MP3_SAMPLE_RATES.get(&sample_rate_idx) {
            Some(val) => val,
            None => continue,
        };

        // padding?
        let has_padding = ((header_num & 0x00000200) >> 9) == 0b1;

        // emphasis
        let emphasis = header_num & 0x00000003;
        if emphasis == 0b10 {
            continue;
        }

        // at this point, it's an MP3 file
        is_mp3 = true;

        // calculate frame length
        let frame_length = (144 * bit_rate / sample_rate
            + match has_padding {
                true => 1,
                false => 0,
            }) as usize;

        // append frame
        if stream.len() < frame_length {
            break;
        }
        let frame_data: Vec<_> = stream.read(frame_length - 4).to_vec();
        mp3_stream.extend(header.clone());
        mp3_stream.extend(frame_data);

        // prepare for next scan-read
        if stream.len() < 3 {
            break;
        }
        header.clear();
        header.push(0);
        header.extend(stream.read(3));
    }

    extracted_mp3s
}
