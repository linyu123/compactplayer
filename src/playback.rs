#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    #[default]
    Sequential,
    RepeatOne,
    RepeatAll,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序播放",
            Self::RepeatOne => "单曲循环",
            Self::RepeatAll => "列表循环",
        }
    }

    pub fn cycle(&mut self) {
        *self = match self {
            Self::Sequential => Self::RepeatOne,
            Self::RepeatOne => Self::RepeatAll,
            Self::RepeatAll => Self::Sequential,
        };
    }

    pub fn next(self, current: usize, count: usize) -> Option<usize> {
        if current >= count {
            return None;
        }
        match self {
            Self::RepeatOne => Some(current),
            Self::RepeatAll => Some((current + 1) % count),
            Self::Sequential => (current + 1 < count).then_some(current + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_boundaries() {
        assert_eq!(Mode::Sequential.next(0, 2), Some(1));
        assert_eq!(Mode::Sequential.next(1, 2), None);
        assert_eq!(Mode::RepeatAll.next(1, 2), Some(0));
        assert_eq!(Mode::RepeatOne.next(1, 2), Some(1));
        for mode in [Mode::Sequential, Mode::RepeatOne, Mode::RepeatAll] {
            assert_eq!(mode.next(0, 0), None);
            assert_eq!(mode.next(2, 2), None);
        }
    }
}
