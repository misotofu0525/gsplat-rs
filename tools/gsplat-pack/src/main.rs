use std::env;
use std::fs;
use std::path::Path;

use gsplat_format::{pack_scene, unpack_scene};
use gsplat_io_ply::load_ply;

fn main() {
    if let Err(err) = run() {
        eprintln!("gsplat-pack failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);

    let input_path = match args.next() {
        Some(path) => path,
        None => {
            return Err(
                "usage: cargo run -p gsplat-pack -- <input.ply> <output.gspk> [--verify]"
                    .to_owned(),
            );
        }
    };

    let output_path = match args.next() {
        Some(path) => path,
        None => {
            return Err(
                "usage: cargo run -p gsplat-pack -- <input.ply> <output.gspk> [--verify]"
                    .to_owned(),
            );
        }
    };

    let verify = args.any(|arg| arg == "--verify");

    let loaded = load_ply(Path::new(&input_path)).map_err(|err| err.to_string())?;
    let blob = pack_scene(&loaded.scene).map_err(|err| err.to_string())?;

    fs::write(&output_path, &blob).map_err(|err| err.to_string())?;

    if verify {
        let unpacked = unpack_scene(&blob).map_err(|err| err.to_string())?;
        if unpacked.len() != loaded.scene.len() {
            return Err("verify failed: gaussian count mismatch".to_owned());
        }
    }

    println!("gsplat-pack complete");
    println!("input={input_path}");
    println!("output={output_path}");
    println!("gaussians={}", loaded.summary.gaussians);
    println!("has_sh_rest={}", loaded.summary.has_sh_rest);
    println!("bytes={}", blob.len());
    println!("verify={verify}");

    Ok(())
}
