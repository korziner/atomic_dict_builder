use clap::Parser;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use anyhow::{Context, Result};

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Создаѥтъ словарь атомарныхъ словъ изъ файла съ длинами (любой пробѣлъ)",
    long_about = r#"
Сіѧ утилита извлекаѥтъ атомарныя (несліянныя) слова изъ файла, гдѣ каждаѧ строка имѣѥтъ форматъ:
    <длина><пробѣлъ><слово>
(напримѣръ, "12 Владимірскій" или "1\tи").

Она рѣшаѥтъ проблему «яйца и курицы»: для разбиенія сліянныхъ словъ (какъ "ВладимірскійиМосковскій")
нуженъ атомарный словарь, но его нѣтъ. Утилита строитъ оный, обрабатываѧ слова отъ кратчайшихъ къ длиннѣйшимъ.
Слово признаѥтся атомарнымъ, егда его не льзѧ разбити на два или болѣе словъ, уже сущихъ во множествѣ.

Все слова суть въ UTF‑8. Алгоритмъ работаетъ на индексахъ буквъ (не байтъ), посему многобайтныѧ кириллическіѧ
литерꙑ (Ѳ, І, Ѣ, ν, ѳ) обрабатываютьсѧ вѣрно.

Обычное примѣненіе: вы имаѥте начальный списокъ словъ, содержащій какъ атомарныѧ, такъ и сліянныѧ формы
(напримѣръ, отъ huniq -c по корпусу). Запустите сію утилиту, дабы получити чистый атомарный словарь,
который потомъ можетъ быти употребленъ разбивателемъ словъ (напр. dp‑segmenter) для раздробленіѧ
длинныхъ составныхъ словъ на ихъ части.

Примѣръ входного файла (длина<таб>слово):
    1   и
    5   иныхъ
    12  Владимірскій
    10  Московскій
    24  ВладимірскійиМосковскій

Порѧдокъ обработки: по числу буквъ (отъ кратчайшихъ). Слово "ВладимірскійиМосковскій"
отметаѥтся, ибо оно разбиваѥтся на "Владимірскій" + "и" + "Московскій", которыѧ уже суть атомарны.
Выходъ содержитъ только атомарныѧ слова (опціонально съ ихъ исходными длинами).

Утилита быстра (O(L²) на слово) и памятливо умѣренна (до 2 милл. словъ требуетъ 500‑800 MB RAM
и исполнѧѥтся за 30‑60 секундъ).
    "#
)]
struct Args {
    /// Входный файлъ (по умолчанію stdin). Форматъ: цѣлоє<пробѣлъ>слово (UTF‑8)
    #[arg(short, long)]
    input: Option<String>,

    /// Выходный файлъ для атомарныхъ словъ (по умолчанію stdout)
    #[arg(short, long)]
    output: Option<String>,

    /// Такожде выводити исходное число (длину) первымъ столбцемъ (чрезъ табулацію)
    #[arg(long)]
    with_numbers: bool,

    /// Наибольшаѧ длина слова **въ буквахъ** для разбиенія (производительность)
    /// Бо́льшіѧ значеніѧ покрывають болѣе словъ, но замедляють работу.
    #[arg(long, default_value = "128")]
    max_word_len: usize,
}



/// Parse a line: returns (number, word) or None.
fn parse_line(line: &str) -> Option<(u64, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // Try tab first
    if let Some(tab_idx) = line.find('\t') {
        let num_str = &line[..tab_idx];
        let word = &line[tab_idx + 1..];
        if let Ok(num) = num_str.parse() {
            return Some((num, word.to_string()));
        }
    }
    // Fallback: split on whitespace
    let mut tokens = line.split_whitespace();
    if let Some(num_str) = tokens.next() {
        if let Ok(num) = num_str.parse() {
            let word = tokens.collect::<Vec<&str>>().join(" ");
            if !word.is_empty() {
                return Some((num, word));
            }
        }
    }
    None
}

/// Check if a word can be split into 2+ dictionary words.
/// Works entirely on `char` indices – no byte slicing of the original string.
fn can_split(word: &str, dict: &HashSet<String>, max_char_len: usize) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    if n == 0 {
        return false;
    }
    let mut dp = vec![false; n + 1];
    dp[0] = true;
    for i in 0..n {
        if !dp[i] {
            continue;
        }
        let max_j = (n - i).min(max_char_len);
        for j in (i + 1)..=i + max_j {
            // Build substring from characters – this is always UTF‑8 safe.
            let subword: String = chars[i..j].iter().collect();
            if dict.contains(&subword) {
                dp[j] = true;
            }
        }
    }
    dp[n]
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read all valid entries
    let input_reader: Box<dyn BufRead> = if let Some(path) = &args.input {
        Box::new(BufReader::new(File::open(path)?))
    } else {
        Box::new(BufReader::new(std::io::stdin()))
    };

    let mut entries = Vec::new();
    for (line_no, line) in input_reader.lines().enumerate() {
        let line = line?;
        if let Some((num, word)) = parse_line(&line) {
            entries.push((num, word));
        } else if !line.trim().is_empty() {
            eprintln!("Warning: line {} malformed: '{}'", line_no + 1, line);
        }
    }
    eprintln!("Read {} valid entries", entries.len());
    if entries.is_empty() {
        eprintln!("No valid entries. Check format: integer<whitespace>word");
        return Ok(());
    }

    // Sort by character length (shortest first), then by number descending
    entries.sort_by(|a, b| {
        let a_len = a.1.chars().count();
        let b_len = b.1.chars().count();
        match a_len.cmp(&b_len) {
            std::cmp::Ordering::Equal => b.0.cmp(&a.0),
            other => other,
        }
    });

    let mut atomic_dict = HashSet::new();
    let mut atomic_entries = Vec::new();
    let max_char_len = args.max_word_len;

    for (num, word) in entries {
        let is_atomic = if word.chars().count() < 2 {
            true
        } else {
            !can_split(&word, &atomic_dict, max_char_len)
        };
        if is_atomic {
            atomic_dict.insert(word.clone());
            atomic_entries.push((num, word));
            if atomic_entries.len() % 100_000 == 0 {
                eprintln!("Processed {} atomic words...", atomic_entries.len());
            }
        }
    }
    eprintln!("Atomic words found: {}", atomic_entries.len());

    // Output
    let mut output_writer: Box<dyn Write> = if let Some(path) = &args.output {
        Box::new(BufWriter::new(File::create(path)?))
    } else {
        Box::new(BufWriter::new(std::io::stdout()))
    };

    if args.with_numbers {
        for (num, word) in atomic_entries {
            writeln!(output_writer, "{}\t{}", num, word)?;
        }
    } else {
        for (_, word) in atomic_entries {
            writeln!(output_writer, "{}", word)?;
        }
    }

    Ok(())
}
