use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn is_media(path: &Path) -> bool {
    is_video(path)
        || matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "mp3"
                | "m4a"
                | "aac"
                | "flac"
                | "wav"
                | "ogg"
                | "opus"
                | "aiff"
                | "aif"
                | "wma"
                | "ac3"
                | "mka"
                | "ape"
                | "alac"
                | "amr"
                | "weba"
        )
}

pub fn is_video(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "mp4"
            | "mkv"
            | "mov"
            | "avi"
            | "webm"
            | "m4v"
            | "flv"
            | "wmv"
            | "mpg"
            | "mpeg"
            | "ts"
            | "mts"
            | "m2ts"
            | "vob"
            | "rm"
            | "rmvb"
            | "3gp"
            | "ogv"
    )
}

pub fn candidates(paths: Vec<PathBuf>) -> Vec<(PathBuf, bool)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut directories = HashSet::new();
    for path in paths {
        let path = path.canonicalize().unwrap_or(path);
        if seen.insert(path.clone()) {
            result.push((path.clone(), true));
        }
        if is_video(&path) {
            if let Some(parent) = path.parent() {
                directories.insert(parent.to_path_buf());
            }
        }
    }
    let mut siblings = Vec::new();
    for directory in directories {
        if let Ok(entries) = directory.read_dir() {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_video(&path) {
                    let path = path.canonicalize().unwrap_or(path);
                    if seen.insert(path.clone()) {
                        siblings.push(path);
                    }
                }
            }
        }
    }
    siblings.sort_by(|left, right| natural_cmp(&left.to_string_lossy(), &right.to_string_lossy()));
    result.extend(siblings.into_iter().map(|path| (path, false)));
    result
}

fn natural_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.to_lowercase();
    let right = right.to_lowercase();
    let mut left = left.as_bytes();
    let mut right = right.as_bytes();
    while !left.is_empty() && !right.is_empty() {
        if left[0].is_ascii_digit() && right[0].is_ascii_digit() {
            let left_end = left.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let right_end = right
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let left_digits = left[..left_end].trim_ascii_start_zero();
            let right_digits = right[..right_end].trim_ascii_start_zero();
            let order = left_digits
                .len()
                .cmp(&right_digits.len())
                .then(left_digits.cmp(right_digits));
            if !order.is_eq() {
                return order;
            }
            left = &left[left_end..];
            right = &right[right_end..];
        } else {
            let order = left[0].cmp(&right[0]);
            if !order.is_eq() {
                return order;
            }
            left = &left[1..];
            right = &right[1..];
        }
    }
    left.len().cmp(&right.len())
}

trait TrimZeros {
    fn trim_ascii_start_zero(&self) -> &[u8];
}
impl TrimZeros for [u8] {
    fn trim_ascii_start_zero(&self) -> &[u8] {
        &self[self.iter().take_while(|byte| **byte == b'0').count()..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn episodes_sort_naturally() {
        assert!(natural_cmp("第2集.mkv", "第10集.mkv").is_lt());
        assert!(natural_cmp("第002集.mkv", "第10集.mkv").is_lt());
    }
    #[test]
    fn rejects_subtitle_and_image_as_siblings() {
        assert!(is_video(Path::new("movie.MKV")));
        assert!(!is_video(Path::new("movie.srt")));
        assert!(!is_video(Path::new("cover.jpg")));
    }
    #[test]
    fn scans_only_sibling_videos_and_deduplicates_aliases() {
        let directory =
            std::env::temp_dir().join(format!("compact-playlist-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for name in [
            "episode1.mkv",
            "episode2.mkv",
            "episode10.mkv",
            "cover.jpg",
            "notes.txt",
        ] {
            std::fs::write(directory.join(name), b"fixture").unwrap();
        }
        std::os::unix::fs::symlink(directory.join("episode1.mkv"), directory.join("alias.mkv"))
            .unwrap();
        let result = candidates(vec![
            directory.join("episode1.mkv"),
            directory.join("alias.mkv"),
            directory.join("./episode1.mkv"),
        ]);
        assert_eq!(result.len(), 3);
        assert!(result[0].1);
        assert_eq!(result[1].0.file_name().unwrap(), "episode2.mkv");
        assert_eq!(result[2].0.file_name().unwrap(), "episode10.mkv");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
