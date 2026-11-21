use std::{fs, path::Path, process::Command};

// 9天体の1日間の暦表。最後の天体だけ開始日をずらせる。
fn data_root(start: f64, last_offset: f64) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("hip.dat"), [0; 45]).unwrap();
    let mut cf = b"CF96".to_vec();
    cf.extend(9_i32.to_le_bytes());
    for index in 0..9 {
        let first = if index == 8 {
            start + last_offset
        } else {
            start
        };
        cf.extend(first.to_le_bytes());
        cf.extend(1_f64.to_le_bytes());
        for n in [0_i32, 1, 1, 0] {
            cf.extend(n.to_le_bytes());
        }
        for _ in 0..3 {
            cf.extend(2_i32.to_le_bytes());
            cf.extend([0; 16]);
            cf.extend(1_i32.to_le_bytes());
            cf.extend(0_i32.to_le_bytes());
        }
    }
    fs::write(root.path().join("cf96.dat"), cf).unwrap();
    root
}

fn inspect(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mitaka-plus"))
        .arg("--inspect")
        .arg("--data-root")
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn inspection_uses_a_date_in_all_planet_ranges_including_shared_endpoints() {
    for (start, offset, expected) in [
        (2451544.5, 0., "mercury_j2000_au="),
        (2455000., 0.5, "jd=2455000.5 mercury_au="),
        (2455000., 1., "jd=2455001 mercury_au="),
    ] {
        let root = data_root(start, offset);
        let output = inspect(root.path());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
    }
}

#[test]
fn inspection_rejects_invalid_constellation_label_and_boundary_declinations() {
    let root = data_root(2451544.5, 0.);
    for (label, boundary) in [(91_f32, 0_f32), (0., -91.)] {
        let mut record = vec![0; 6]; // 空の名前を6個
        record.extend(0_f32.to_le_bytes());
        record.extend(label.to_le_bytes());
        record.push(0); // 星座線の数
        record.extend(1_i16.to_le_bytes());
        record.extend(0_f32.to_le_bytes());
        record.extend(boundary.to_le_bytes());
        fs::write(root.path().join("constellation_HIP.dat"), record).unwrap();
        let output = inspect(root.path());
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("星座レコード 1 の赤緯"));
    }
}
