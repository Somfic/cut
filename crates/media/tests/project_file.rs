use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use cut_timeline::file::{load, save};
use cut_timeline::{Edit, Source, Timeline};

const NTSC: (&str, &str, usize) = ("ntsc.mp4", "30000/1001", 60);
const PAL: (&str, &str, usize) = ("pal.mp4", "25/1", 50);

fn probe(path: &Path) -> anyhow::Result<Source> {
    cut_media::probe_source(path)
}

/// Two tiny clips beside a project file: the rates have to come back off
/// a real probe for a round trip to mean anything.
fn fixture(name: &str) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("cut-{name}"));
    fs::create_dir_all(&dir).ok()?;

    for (file, frame_rate, buffers) in [NTSC, PAL] {
        let target = dir.join(file);

        let status = Command::new("gst-launch-1.0")
            .args([
                "-q",
                "videotestsrc",
                &format!("num-buffers={buffers}"),
                "!",
                &format!("video/x-raw,width=320,height=240,framerate={frame_rate}"),
                "!",
                "x264enc",
                "!",
                "mp4mux",
                "!",
                "filesink",
                &format!("location={}", target.display()),
            ])
            .status();

        match status {
            Ok(status) if status.success() && target.exists() => {}
            _ => {
                eprintln!("skipping: could not encode {}", target.display());
                return None;
            }
        }
    }

    Some(dir)
}

#[test]
fn round_trips_a_timeline() {
    cut_media::init().unwrap();

    let Some(dir) = fixture("round-trip") else {
        return;
    };
    let ntsc = Arc::new(probe(&dir.join(NTSC.0)).unwrap());
    let pal = Arc::new(probe(&dir.join(PAL.0)).unwrap());

    // The same source twice: the round trip has to collapse it to one
    // entry and share the `Arc` again.
    let mut before = Timeline::default();
    for (source, position, source_start, length) in [
        (ntsc.clone(), 0, 4, 10),
        (pal.clone(), 10, 0, 20),
        (ntsc.clone(), 30, 20, 15),
    ] {
        before
            .apply(Edit::Place {
                track: 0,
                source,
                position,
                source_start,
                length,
            })
            .unwrap();
    }

    let path = dir.join("project.cut");
    save(&before, &path).unwrap();
    let after = load(&path, &probe).unwrap();

    assert_eq!(after.timebase, ntsc.frame_rate);
    assert_eq!(after.tracks.len(), 1);

    let restored = &after.tracks[0].clips;
    let original = &before.tracks[0].clips;
    assert_eq!(restored.len(), original.len());

    for (restored, original) in restored.iter().zip(original) {
        assert_eq!(restored.position, original.position);
        assert_eq!(restored.source_start, original.source_start);
        assert_eq!(restored.length, original.length);
        assert_eq!(restored.source.path, original.source.path);
        assert_eq!(restored.source.frame_rate, original.source.frame_rate);
    }

    assert!(Arc::ptr_eq(&restored[0].source, &restored[2].source));

    // Two sources for three clips, addressed relative to the project, at
    // an exact rate rather than 29.97.
    let json = fs::read_to_string(&path).unwrap();
    assert_eq!(json.matches(".mp4").count(), 2);
    assert!(json.contains("\"ntsc.mp4\""));
    assert!(json.contains("30000"), "{json}");
}

#[test]
fn keeps_a_track_switched_off() {
    cut_media::init().unwrap();

    let Some(dir) = fixture("switched-off") else {
        return;
    };
    let ntsc = Arc::new(probe(&dir.join(NTSC.0)).unwrap());

    let mut before = Timeline::default();
    before
        .apply(Edit::Place {
            track: 0,
            source: ntsc,
            position: 0,
            source_start: 0,
            length: 10,
        })
        .unwrap();
    before
        .apply(Edit::Enable {
            track: 0,
            enabled: false,
        })
        .unwrap();

    let path = dir.join("off.cut");
    save(&before, &path).unwrap();

    assert!(!load(&path, &probe).unwrap().tracks[0].enabled);
}
