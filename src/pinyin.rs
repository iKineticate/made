use ib_pinyin::{matcher::PinyinMatcher, pinyin::PinyinNotation};

pub fn match_pinyin(input: &str, text: &str) -> bool {
    let matcher = PinyinMatcher::builder(input)
        .pinyin_notations(PinyinNotation::Ascii | PinyinNotation::AsciiFirstLetter)
        .build();

    matcher.is_match(text)
}
