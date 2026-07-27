mod cli;
mod image_output;
mod offscreen;
mod scene;
#[cfg(feature = "interactive-viewer")]
mod surface_evidence;
#[cfg(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32")))]
mod surface_sustained;
mod trace;
mod viewer;

use std::env;

use cli::Args;

fn main() {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    if let Err(err) = run(args) {
        eprintln!("desktop-example failed: {err}");
        std::process::exit(1);
    }
}

fn run(mut args: Args) -> Result<(), String> {
    let trace_playback = trace::load_camera_trace(&mut args)?;
    cli::validate_surface_trace_geometry(&args, trace_playback.is_some())?;
    if args.interactive {
        viewer::run(&args, trace_playback.as_ref())
    } else {
        offscreen::run_offscreen(&args, trace_playback.as_ref())
    }
}

#[cfg(test)]
mod tests;
