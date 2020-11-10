use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Pixel};
use itertools::Itertools;
use rayon::prelude::*;
use std::convert::TryFrom;
use std::error::Error;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opts {
    #[structopt(required = true, parse(from_os_str))]
    input: PathBuf,
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

enum OnOffRule {
    PxThreshold(i32),
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

fn main() -> Result<(), Box<dyn Error>> {
    let opts: Opts = Opts::from_args();
    let img = image::open(opts.input)?;

    let (width, height) = img.dimensions();

    const RV: u32 = 1;
    const RH: u32 = 1;

    let img = img.resize(width / RH, height / RV, FilterType::Gaussian);
    let (width, height) = img.dimensions();

    let rl = OnOffRule::PxThreshold(75);

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
