use rayon::prelude::*;
use std::convert::TryFrom;
use std::env;
use std::ffi::OsString;
use std::fs;

const THRESHOLD: usize = 50 * (1 << 10);  // 50 KiB
static MP3_BIT_RATES: [u32; 14] = [
    32000, 40000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 160000, 192000, 224000,
    256000, 320000,
];
static MP3_SAMPLE_RATES: [u32; 3] = [44100, 48000, 3200];

fn get_bit_rate(i: u32) -> Option<&'static u32> {
    let min = 0b0001;
    let i = i.checked_sub(min)?;
    let i = usize::try_from(i).ok()?;
    MP3_BIT_RATES.get(i)
}

fn get_sample_rate(i: u32) -> Option<&'static u32> {
    let min = 0b00;
    let i = i.checked_sub(min)?;
    let i = usize::try_from(i).ok()?;
    MP3_SAMPLE_RATES.get(i)
}

fn main() {
    let args = env::args_os().collect::<Vec<_>>();
    args[1..].par_iter().for_each(|f| process_file(f));
}

fn process_file(file: &OsString) {
    // deobfuscate files and extract mp3s
    let buffer: Vec<u8> = match fs::read(file) {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Error opening {:?}: {}", file, err);
            return;
        }
    };
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
        let suffix = format!(".{}.mp3", i + 1);
        let mut outfile = file.clone();
        outfile.push(suffix);
        println!("writing {:?}", outfile);
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
    let stream_iter = &mut s.iter();
    let total_stream_len: usize = stream_iter.len();

    // return value
    let mut extracted_mp3s: Vec<(Vec<u8>, usize)> = Vec::new();

    let mut header: Vec<u8> = vec![0];
    let data = stream_iter.take(3);
    if data.len() < 3 {
        return extracted_mp3s;
    }
    header.extend(data);

    let mut mp3_stream: Vec<u8> = Vec::new();
    mp3_stream.reserve(total_stream_len);
    let mut is_mp3 = false;

    loop {
        if !is_mp3 {
            if mp3_stream.len() > THRESHOLD {
                let offset: usize = total_stream_len - stream_iter.len() - mp3_stream.len();
                extracted_mp3s.push((mp3_stream.clone(), offset));
            }
            mp3_stream.clear();
        }

        is_mp3 = false;

        // read header_num
        header.remove(0);
        let x = *match stream_iter.next() {
            Some(v) => v,
            None => break,
        };
        header.push(x);
        let header_num = u32::from(header[0]) << 24
            | u32::from(header[1]) << 16
            | u32::from(header[2]) << 8
            | u32::from(header[3]);

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
        let bit_rate = match get_bit_rate(bit_rate_idx) {
            Some(val) => val,
            None => continue,
        };

        // sample rate
        let sample_rate_idx = (header_num & 0x00000C00) >> 10;
        if sample_rate_idx == 0b11 {
            continue;
        }
        let sample_rate = match get_sample_rate(sample_rate_idx) {
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
        let frame_data = stream_iter.take(frame_length - 4);
        if frame_data.len() < frame_length - 4 {
            break;
        }
        mp3_stream.extend(header.clone());
        mp3_stream.extend(frame_data);

        // prepare for next scan-read
        header.clear();
        header.push(0);
        let data = stream_iter.take(3);
        if data.len() < 3 {
            break;
        }
        header.extend(data);
    }

    extracted_mp3s
}
