use super::rng::Rng;
use gtk::glib;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub fn expand(text: &str, (width, height): (i32, i32)) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let text = replace_random_macros(text);
    let Ok(now) = glib::DateTime::now_local() else {
        return text.to_owned();
    };
    let twelve_hour = text.contains("%pm");
    let hour = match (twelve_hour, now.hour()) {
        (true, 0) => 12,
        (true, h) if h > 12 => h - 12,
        (_, h) => h,
    };
    let local = |format: &str| {
        now.format(format)
            .map(|s| s.to_string())
            .unwrap_or_default()
    };
    let nonzero = |n: i32| if n == 0 { String::new() } else { n.to_string() };
    let macros = [
        ("%width", nonzero(width)),
        ("%height", nonzero(height)),
        ("%mon2", MONTHS[now.month() as usize - 1].to_owned()),
        ("%mon", local("%B")),
        ("%yy", format!("{:02}", now.year() % 100)),
        ("%y", now.year().to_string()),
        ("%mo", format!("{:02}", now.month())),
        ("%d", format!("{:02}", now.day_of_month())),
        ("%h", format!("{hour:02}")),
        ("%mi", format!("{:02}", now.minute())),
        ("%s", format!("{:02}", now.second())),
        ("%wy", format!("{:02}", now.week_of_year())),
        ("%w2", DAYS[now.day_of_week() as usize - 1].to_owned()),
        ("%w", local("%A")),
        ("%pm", if now.hour() >= 12 { "PM" } else { "AM" }.to_owned()),
        ("%unix", now.to_unix().to_string()),
        ("%un", glib::user_name().to_string_lossy().into_owned()),
        ("%cn", glib::host_name().to_string()),
        ("%n", "\n".to_owned()),
    ];
    macros.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(name, value)
    })
}

enum RandomSource {
    Characters(&'static str),
    Words(&'static [&'static str]),
}

const DIGITS: &str = "0123456789";
const LETTERS_AND_DIGITS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const HEX_DIGITS: &str = "0123456789ABCDEF";
const ADJECTIVES: &[&str] = &[
    "amber", "brave", "bright", "calm", "clever", "crisp", "daring", "eager", "fancy", "gentle",
    "happy", "jolly", "keen", "lively", "mellow", "noble", "polite", "quick", "quiet", "rapid",
    "shiny", "silent", "sleepy", "smooth", "snappy", "sunny", "swift", "tidy", "vivid", "witty",
];
const ANIMALS: &[&str] = &[
    "badger", "beaver", "camel", "dolphin", "eagle", "falcon", "gecko", "heron", "ibis", "jaguar",
    "koala", "lemur", "lynx", "marmot", "newt", "otter", "panda", "quail", "raven", "seal",
    "tiger", "urchin", "viper", "walrus", "yak", "zebra",
];

const RANDOM_MACROS: [(&str, RandomSource); 5] = [
    ("radjective", RandomSource::Words(ADJECTIVES)),
    ("ranimal", RandomSource::Words(ANIMALS)),
    ("rn", RandomSource::Characters(DIGITS)),
    ("ra", RandomSource::Characters(LETTERS_AND_DIGITS)),
    ("rx", RandomSource::Characters(HEX_DIGITS)),
];

const LONGEST_REPETITION: usize = 64;

fn fingerprint(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn repetition_after(name_end: &str) -> Option<(usize, usize)> {
    let digits = name_end.strip_prefix('{')?;
    let closing = digits.find('}')?;
    let count = digits[..closing].parse::<usize>().ok()?;
    Some((count.clamp(1, LONGEST_REPETITION), closing + 2))
}

fn draw(source: &RandomSource, rng: &mut Rng) -> String {
    match source {
        RandomSource::Characters(characters) => {
            let all: Vec<char> = characters.chars().collect();
            all[rng.pick(all.len())].to_string()
        }
        RandomSource::Words(words) => words[rng.pick(words.len())].to_owned(),
    }
}

fn replace_random_macros(text: &str) -> String {
    let mut rng = Rng::seeded(fingerprint(text));
    let mut result = String::new();
    let mut rest = text;
    while let Some(percent) = rest.find('%') {
        result.push_str(&rest[..percent]);
        rest = &rest[percent + 1..];
        let found = RANDOM_MACROS
            .iter()
            .find(|(name, _)| rest.starts_with(name));
        let Some((name, source)) = found else {
            result.push('%');
            continue;
        };
        rest = &rest[name.len()..];
        let (repetitions, consumed) = repetition_after(rest).unwrap_or((1, 0));
        rest = &rest[consumed..];
        for _ in 0..repetitions {
            result.push_str(&draw(source, &mut rng));
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_size_macros_and_keeps_unknown_ones() {
        assert_eq!(expand("%width × %heightpx", (640, 480)), "640 × 480px");
        assert_eq!(expand("plain", (1, 1)), "plain");
        assert_eq!(expand("100%", (1, 1)), "100%");
        let year = expand("%y", (1, 1));
        assert_eq!(year.len(), 4);
    }

    #[test]
    fn random_macros_draw_the_requested_kind_and_count() {
        let digits = expand("%rn{5}", (1, 1));
        assert_eq!(digits.len(), 5);
        assert!(digits.chars().all(|c| c.is_ascii_digit()));
        let name = expand("%radjective.bmp", (1, 1));
        assert!(name.ends_with(".bmp") && !name.contains('%'));
        let hex = expand("%rx{8}", (1, 1));
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn random_macros_are_stable_for_the_same_text() {
        assert_eq!(expand("1%rn.%rnK", (1, 1)), expand("1%rn.%rnK", (1, 1)));
        assert_eq!(expand("100%", (1, 1)), "100%");
    }
}
