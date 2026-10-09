fn main() {
    if let Some(code) = release_command() {
        std::process::exit(code);
    }
    #[cfg(windows)]
    shixu_desktop::runtime::run();
    #[cfg(not(windows))]
    {
        eprintln!("UNSUPPORTED: Windows desktop runtime required");
        std::process::exit(1);
    }
}
// These two explicit commands never start a WebView, vault, provider or QQ worker.
fn release_command() -> Option<i32> {
    use sha2::{Digest, Sha256};
    use shixu_desktop::release::{MAX_EVIDENCE_BYTES, compiled_manifest, hex, validate};
    use std::io::{Read, Write};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        return None;
    }
    if args[0] != "--release-manifest" && args[0] != "--verify-release" {
        return Some(1);
    }
    let artifact = (|| -> std::io::Result<String> {
        let mut file = std::fs::File::open(std::env::current_exe()?)?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        Ok(hex(&hash.finalize()))
    })();
    let Ok(artifact) = artifact else {
        eprintln!("FAIL: executable identity unavailable");
        return Some(1);
    };
    if args[0] == "--release-manifest" {
        if args.len() != 1 {
            return Some(1);
        }
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"manifest":compiled_manifest(),"artifact_sha256":artifact})
            )
            .unwrap()
        );
        return Some(0);
    }
    if args.len() != 3 || args[1] == args[2] {
        eprintln!("FAIL: expected --verify-release EVIDENCE_OR_DASH OUTPUT");
        return Some(1);
    }
    let raw = if args[1] == "-" {
        vec![]
    } else {
        match std::fs::File::open(&args[1]).and_then(|f| {
            let mut bytes = vec![];
            f.take((MAX_EVIDENCE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        }) {
            Ok(bytes) => bytes,
            Err(_) => b"{}".to_vec(),
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(-1);
    let report = validate(&raw, &artifact, now);
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    // create_new atomically refuses every existing destination, including links.
    // Write through the opened handle; never check a path and then truncate it.
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])
        .and_then(|mut file| file.write_all(&bytes));
    if written.is_err() {
        eprintln!("FAIL: release report requires a new writable output file");
        return Some(1);
    }
    eprintln!(
        "Release verdict: {:?}; required blockers: {}",
        report.verdict,
        report.blockers.len()
    );
    Some(report.exit_code())
}
