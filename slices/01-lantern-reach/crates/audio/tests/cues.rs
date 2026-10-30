//! Quality checks without ears: every take of every cue is rendered and measured
//! against the design limits (`galaxy_audio::Limits`). No audio device needed.

use galaxy_audio::{Cue, Limits, render_all};

#[test]
fn every_cue_is_soft_warm_and_short() {
    let reports = render_all(None).unwrap();
    for r in &reports {
        let m = r.metrics;
        println!(
            "{:<10} take {}  {:.3} s  peak {:6.1}  rms {:6.1}  centroid {:5.0} Hz  harsh {:.4}  rise {:.1} ms  dc {:+.6}  edge {:.5}",
            r.cue.name(),
            r.take,
            m.duration,
            m.peak_db,
            m.rms_db,
            m.centroid_hz,
            m.harsh_share,
            m.rise * 1000.0,
            m.dc,
            m.edge
        );
    }
    let bad: Vec<_> = reports
        .iter()
        .filter(|r| !r.problems.is_empty())
        .map(|r| {
            format!(
                "{} take {}: {}",
                r.cue.name(),
                r.take,
                r.problems.join(", ")
            )
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
    // Every cue rendered every take.
    for cue in Cue::ALL {
        let n = reports.iter().filter(|r| r.cue == cue).count();
        assert_eq!(n, cue.spec().patch.takes as usize, "{}", cue.name());
    }
}

#[test]
fn the_frequent_cues_are_the_quiet_ones() {
    let reports = render_all(None).unwrap();
    let loudest = |cue: Cue| {
        reports
            .iter()
            .filter(|r| r.cue == cue)
            .map(|r| r.metrics.rms_db)
            .fold(f32::MIN, f32::max)
    };
    // The ore tick sits well under the chimes, and the UI under the world's chimes.
    assert!(loudest(Cue::OreTick) + 6.0 < loudest(Cue::Dock));
    assert!(loudest(Cue::Confirm) < loudest(Cue::Dock));
    let _ = Limits::of(Cue::OreTick);
}

#[test]
fn render_writes_one_wav_per_take() {
    let dir = tempfile::tempdir().unwrap();
    let reports = render_all(Some(dir.path())).unwrap();
    for r in &reports {
        let file = r.file.as_ref().unwrap();
        let len = std::fs::metadata(file).unwrap().len();
        assert!(len > 44, "{}", file.display());
    }
    let wavs = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(wavs, reports.len());
}
