use clap::Clap;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Pixel};
use itertools::Itertools;
use rayon::prelude::*;
use regex::Regex;
use std::convert::TryFrom;
use std::error::Error;
use std::num::ParseIntError;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

enum ImageSize {
    Default,

    Sized { width: u32, height: u32 },
}

#[derive(Error, Debug)]
enum ImageSizeParseError {
    #[error("couldn't parse an int in the image size")]
    ParseIntError(#[from] ParseIntError),
    #[error("unknown size format `{0}`")]
    UnknownSizeFormat(String),
}

impl FromStr for ImageSize {
    type Err = ImageSizeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "_" {
            return Ok(Self::Default);
        }
        let mut it = s.split("x");
        if let Some(w_str) = it.next() {
            if let Some(h_str) = it.next() {
                return Ok(Self::Sized {
                    width: u32::from_str(w_str)?,
                    height: u32::from_str(h_str)?,
                });
            }
        }

        Err(ImageSizeParseError::UnknownSizeFormat(s.into()))
    }
}

#[derive(Clap)]
struct Opts {
    #[clap(required = true, parse(from_os_str))]
    input: PathBuf,

    #[clap(short, long, default_value = "_", parse(try_from_str))]
    size: ImageSize,

    #[clap(short, long, default_value = "Threshold(100)", parse(try_from_str))]
    rule: OnOffRule,
}

/// UTF8 of first (empty) braille character
const OFF_0: u32 = 0x2800;

fn region_braille<F>(x: u32, y: u32, f: F) -> u32
where
    F: Fn((u32, u32)) -> Option<bool>,
{
    OFF_0
        + [
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 0),
            (3, 1),
        ]
        .iter()
        .map(|&(dy, dx)| (y * 4 + dy, x * 2 + dx))
        .enumerate()
        .map(|(index, v)| {
            // println!("index: {}, off: {:?}", index, v);
            ((f(v).unwrap_or(false) as u8) << index) as u32
        })
        .sum::<u32>()
}

#[derive(Copy, Clone)]
enum OnOffRule {
    PxThreshold(i32),
    InvertedPxThreshold(i32),
    Border(i32, i32),
}

fn absdiff(a: u8, b: u8) -> u8 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

impl OnOffRule {
    fn is_on(&self, img: &DynamicImage, x: u32, y: u32) -> bool {
        if !img.in_bounds(x, y) {
            return false;
        }
        match self {
            OnOffRule::PxThreshold(threshold) => {
                *threshold <= img.get_pixel(x, y).0.iter().map(|&v| v as i32).sum::<i32>()
            }
            OnOffRule::InvertedPxThreshold(threshold) => {
                *threshold
                    >= img
                        .get_pixel(x, y)
                        .to_rgb()
                        .0
                        .iter()
                        .map(|&v| v as i32)
                        .sum::<i32>()
            }
            OnOffRule::Border(threshold, distance) => {
                let px = img.get_pixel(x, y);

                [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .cartesian_product(1..=*distance)
                    .map(|(&(dx, dy), d)| (dx * d, dy * d))
                    .any(|(dx, dy)| {
                        let nx = u32::try_from(x as i32 + dx).unwrap_or(0);
                        let ny = u32::try_from(y as i32 + dy).unwrap_or(0);
                        if !img.in_bounds(nx, ny) {
                            return false;
                        }

                        let df = img
                            .get_pixel(nx, ny)
                            .0
                            .iter()
                            .zip(px.0.iter())
                            .map(|(&a, &b)| absdiff(a, b) as i32)
                            .max()
                            .unwrap_or(0);

                        df >= *threshold
                    })
            }
        }
    }
}

#[derive(Error, Debug)]
enum OnOffRuleParseError {
    #[error("number parse error")]
    ParseIntError(#[from] ParseIntError),

    #[error("unknown format for on off rule: `{0}`")]
    UnknownFormat(String),
}

impl FromStr for OnOffRule {
    type Err = OnOffRuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^Threshold\((\d+)\)$").unwrap();

        if re.is_match(s) {
            let thr = re.captures(s).unwrap().iter().nth(1).unwrap().unwrap();

            return Ok(OnOffRule::PxThreshold(i32::from_str(thr.as_str())?));
        }

        let re = Regex::new(r"^InvertedThreshold\((\d+)\)$").unwrap();

        if re.is_match(s) {
            let thr = re.captures(s).unwrap().iter().nth(1).unwrap().unwrap();

            return Ok(OnOffRule::InvertedPxThreshold(i32::from_str(thr.as_str())?));
        }

        let re = Regex::new(r"^Border\((\d+),(\d+)\)$").unwrap();

        if re.is_match(s) {
            let captures = re.captures(s).unwrap();
            let mut captures_iter = captures.iter();
            let border_threshold = captures_iter.nth(1).unwrap().unwrap();
            let border_size = captures_iter.next().unwrap().unwrap();

            return Ok(OnOffRule::Border(
                i32::from_str(border_threshold.as_str())?,
                i32::from_str(border_size.as_str())?,
            ));
        }

        Err(OnOffRuleParseError::UnknownFormat(s.into()))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let opts: Opts = Opts::parse();
    let img = image::open(opts.input)?;

    let img = match &opts.size {
        ImageSize::Default => img,
        ImageSize::Sized { width, height } => {
            if *width != img.width() || *height != img.height() {
                img.resize(*width, *height, FilterType::Triangle)
            } else {
                img
            }
        }
    };
    let (width, height) = img.dimensions();

    let rl = opts.rule;

    let mat: Vec<Vec<bool>> = (0..height)
        .map(|y| {
            (0..width)
                .into_par_iter()
                .map(|x| rl.is_on(&img, x, y))
                .collect()
        })
        .collect();

    (0..=height / 4).for_each(|y| {
        (0..=width / 2).for_each(|x| {
            let v = region_braille(x, y, |(y, x)| {
                if !img.in_bounds(x, y) {
                    return None;
                }

                Some(mat[y as usize][x as usize])
            });

            let chr = std::char::from_u32(v).unwrap();

            print!("{}", chr);
        });
        println!()
    });

    Ok(())
}
