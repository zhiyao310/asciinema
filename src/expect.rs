use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use nix::sys::wait::WaitStatus;
use regex::Regex;
use tokio::time::{sleep, timeout};

use crate::asciicast::{Event, Header, V3Encoder};
use crate::cli::Expect;
use crate::pty::{self, Pty};
use crate::status;
use crate::tty::TtySize;
use crate::util::Utf8Decoder;

mod monitor;

use monitor::Monitor;

const EXPECT_BUFFER_LIMIT: usize = 1024 * 1024;

trait ExecutionObserver {
    fn pty_size(&self) -> Option<TtySize> {
        None
    }

    fn output(&mut self, _bytes: &[u8]) -> Result<()> {
        Ok(())
    }

    fn instruction(&mut self, _progress: Progress<'_>) -> Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

struct Progress<'a> {
    current: usize,
    total: usize,
    line: usize,
    label: &'a str,
    summary: &'a str,
}

struct NullObserver;

impl ExecutionObserver for NullObserver {}

impl Expect {
    pub fn run(self) -> Result<ExitCode> {
        let paths = BuildPaths::new(&self.script, self.output_dir.as_deref())?;
        let instructions = parse_script(&self.script)?;

        status::highlight!("Running scenario from {}", self.script.display());
        fs::create_dir_all(&paths.output_dir).with_context(|| {
            format!(
                "can't create output directory {}",
                paths.output_dir.to_string_lossy()
            )
        })?;

        let runtime = tokio::runtime::Runtime::new()?;
        status::highlight!("Recording terminal session to {}", paths.cast.display());
        let recording_result =
            runtime.block_on(record(&self, &self.script, &paths.cast, instructions));
        let recording = recording_result?;

        if recording.exit_status != 0 {
            bail!(
                "scenario shell exited with status {}",
                recording.exit_status
            );
        }

        let theme = parse_theme(&self.theme)?;
        status::highlight!("Rendering GIF to {}", paths.gif.display());
        render_gif(&paths.cast, &paths.gif, theme.clone(), self.font_size, None)?;

        if !recording.snapshot_labels.is_empty() {
            status::highlight!(
                "Exporting {} snapshot(s) to {}",
                recording.snapshot_labels.len(),
                paths.snapshot_dir.display()
            );
            render_snapshot_pngs(
                &paths,
                theme,
                self.font_size,
                &self.ffmpeg,
                &recording.snapshot_labels,
            )?;
        }

        status::highlight!("Encoding MP4 to {}", paths.mp4.display());
        encode_mp4(&self.ffmpeg, &paths.gif, &paths.mp4)?;
        status::highlight!("Artifacts written to {}", paths.output_dir.display());

        Ok(ExitCode::SUCCESS)
    }
}

struct BuildPaths {
    output_dir: PathBuf,
    cast: PathBuf,
    gif: PathBuf,
    mp4: PathBuf,
    snapshot_dir: PathBuf,
    stem: String,
}

impl BuildPaths {
    fn new(script: &Path, output_dir: Option<&Path>) -> Result<Self> {
        if script.extension().is_none_or(|extension| extension != "sh") {
            bail!("expect scenario must use a .sh extension");
        }

        let stem = script
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| anyhow!("scenario path has no valid file stem"))?
            .to_owned();
        let parent = script.parent().unwrap_or_else(|| Path::new("."));
        let output_dir = output_dir
            .map(Path::to_owned)
            .unwrap_or_else(|| parent.join(&stem));

        Ok(Self {
            cast: output_dir.join(format!("{stem}.cast")),
            gif: output_dir.join(format!("{stem}.gif")),
            mp4: output_dir.join(format!("{stem}.mp4")),
            snapshot_dir: output_dir.join("snapshots"),
            output_dir,
            stem,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Instruction {
    Command(String),
    Delay(Duration),
    Expect(String),
    Snapshot(String),
    Send(String),
    SendCharacter(String),
    SendArrow {
        direction: Arrow,
        count: usize,
        enter: bool,
    },
    SendControl(char),
    Wait(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Arrow {
    Down,
    Left,
    Right,
    Up,
}

impl Arrow {
    fn bytes(self) -> &'static [u8] {
        match self {
            Arrow::Down => b"\x1b[B",
            Arrow::Left => b"\x1b[D",
            Arrow::Right => b"\x1b[C",
            Arrow::Up => b"\x1b[A",
        }
    }
}

struct ParsedScript {
    instructions: Vec<SourceInstruction>,
}

struct SourceInstruction {
    line: usize,
    instruction: Instruction,
}

impl ParsedScript {
    fn push(&mut self, line: usize, instruction: Instruction) {
        self.instructions
            .push(SourceInstruction { line, instruction });
    }
}

fn parse_script(path: &Path) -> Result<ParsedScript> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("can't read scenario {}", path.to_string_lossy()))?;
    parse_script_with_lines(&content)
}

#[cfg(test)]
fn parse_script_content(content: &str) -> Result<Vec<Instruction>> {
    Ok(parse_script_with_lines(content)?
        .instructions
        .into_iter()
        .map(|instruction| instruction.instruction)
        .collect())
}

fn parse_script_with_lines(content: &str) -> Result<ParsedScript> {
    let mut parsed = ParsedScript {
        instructions: Vec::new(),
    };
    let mut continued_command = String::new();
    let mut continued_line = None;
    let mut lines = content.lines().enumerate();

    while let Some((index, original_line)) = lines.next() {
        let line_number = index + 1;
        let line = original_line.trim_end();
        let trimmed = line.trim_start();

        if let Some(directive) = trimmed.strip_prefix("#$") {
            if !continued_command.is_empty() {
                bail!("line {line_number}: directive interrupts a continued command");
            }

            parsed.push(
                line_number,
                parse_directive(directive.trim_start(), line_number)?,
            );
        } else if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        } else if let Some(delimiter) = heredoc_delimiter(line) {
            if !continued_command.is_empty() {
                bail!("line {line_number}: heredoc interrupts a continued command");
            }

            let mut body = String::new();
            loop {
                let Some((_, body_line)) = lines.next() else {
                    bail!("line {line_number}: unterminated heredoc {delimiter:?}");
                };
                body.push_str(body_line);
                body.push('\n');

                if body_line == delimiter {
                    break;
                }
            }

            parsed.push(line_number, Instruction::Command(line.to_owned()));
            parsed.push(line_number, Instruction::Send(body));
        } else if line.ends_with('\\') {
            if continued_command.is_empty() {
                continued_line = Some(line_number);
            }
            continued_command.push_str(line);
            continued_command.push('\n');
        } else {
            continued_command.push_str(line);
            parsed.push(
                continued_line.take().unwrap_or(line_number),
                Instruction::Command(std::mem::take(&mut continued_command)),
            );
        }
    }

    if !continued_command.is_empty() {
        bail!(
            "line {}: scenario ends with a continued command",
            continued_line.unwrap_or_default()
        );
    }

    Ok(parsed)
}

fn heredoc_delimiter(line: &str) -> Option<String> {
    let (_, delimiter) = line.rsplit_once("<<")?;
    let delimiter = delimiter
        .trim()
        .strip_prefix('-')
        .unwrap_or(delimiter.trim());

    if delimiter.is_empty() || delimiter.split_whitespace().count() != 1 {
        return None;
    }

    let delimiter = if (delimiter.starts_with('\'') && delimiter.ends_with('\''))
        || (delimiter.starts_with('"') && delimiter.ends_with('"'))
    {
        &delimiter[1..delimiter.len() - 1]
    } else {
        delimiter
    };

    (!delimiter.is_empty()).then(|| delimiter.to_owned())
}

fn parse_directive(directive: &str, line_number: usize) -> Result<Instruction> {
    let parse_millis = |value: &str| -> Result<Duration> {
        let milliseconds = value
            .parse::<u64>()
            .with_context(|| format!("line {line_number}: expected milliseconds"))?;
        Ok(Duration::from_millis(milliseconds))
    };

    if let Some(value) = directive.strip_prefix("delay ") {
        Ok(Instruction::Delay(parse_millis(value)?))
    } else if let Some(value) = directive.strip_prefix("expect ") {
        let value = decode_escapes(value)?;
        Regex::new(&value).with_context(|| format!("line {line_number}: invalid regex"))?;
        Ok(Instruction::Expect(value))
    } else if let Some(value) = directive.strip_prefix("snapshot ") {
        Ok(Instruction::Snapshot(value.trim().to_owned()))
    } else if let Some(value) = directive.strip_prefix("sendcharacter ") {
        Ok(Instruction::SendCharacter(value.to_owned()))
    } else if let Some(value) = directive.strip_prefix("send ") {
        Ok(Instruction::Send(decode_escapes(value)?))
    } else if let Some(value) = directive.strip_prefix("sendcontrol ") {
        let mut chars = value.chars();
        let control = chars
            .next()
            .filter(|_| chars.next().is_none())
            .ok_or_else(|| anyhow!("line {line_number}: sendcontrol expects one character"))?;
        Ok(Instruction::SendControl(control))
    } else if let Some(value) = directive.strip_prefix("sendarrow ") {
        parse_arrow(value, false, line_number)
    } else if let Some(value) = directive.strip_prefix("sendlinearrow ") {
        parse_arrow(value, true, line_number)
    } else if let Some(value) = directive.strip_prefix("wait ") {
        Ok(Instruction::Wait(parse_millis(value)?))
    } else {
        bail!("line {line_number}: unknown expect directive {directive:?}")
    }
}

// Match the escape subset accepted by the original Python automation parser.
// Unknown escapes stay literal so regex escapes such as `\$` keep their meaning.
fn decode_escapes(value: &str) -> Result<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();

    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }

        let Some(escaped) = characters.next() else {
            decoded.push('\\');
            break;
        };

        let simple = match escaped {
            '\\' => Some('\\'),
            '\'' => Some('\''),
            '"' => Some('"'),
            'a' => Some('\x07'),
            'b' => Some('\x08'),
            'f' => Some('\x0c'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\x0b'),
            _ => None,
        };
        if let Some(character) = simple {
            decoded.push(character);
            continue;
        }

        let digits = match escaped {
            'x' => take_digits(&mut characters, 2, 16),
            'u' => take_digits(&mut characters, 4, 16),
            'U' => take_digits(&mut characters, 8, 16),
            '0'..='7' => {
                let mut digits = String::from(escaped);
                while digits.len() < 3 {
                    match characters.peek().copied() {
                        Some('0'..='7') => digits.push(characters.next().unwrap()),
                        _ => break,
                    }
                }
                Some((digits, 8))
            }
            _ => None,
        };

        if let Some((digits, radix)) = digits {
            let codepoint = u32::from_str_radix(&digits, radix)
                .with_context(|| format!("invalid escape sequence \\{escaped}{digits}"))?;
            let character = char::from_u32(codepoint)
                .ok_or_else(|| anyhow!("invalid Unicode codepoint in escape sequence"))?;
            decoded.push(character);
        } else {
            decoded.push('\\');
            decoded.push(escaped);
        }
    }

    Ok(decoded)
}

fn take_digits(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    length: usize,
    radix: u32,
) -> Option<(String, u32)> {
    let digits = characters.clone().take(length).collect::<String>();
    if digits.chars().count() != length
        || digits
            .chars()
            .any(|character| character.to_digit(radix).is_none())
    {
        return None;
    }

    for _ in 0..length {
        characters.next();
    }

    Some((digits, radix))
}

fn parse_arrow(value: &str, enter: bool, line_number: usize) -> Result<Instruction> {
    let mut parts = value.split_whitespace();
    let direction = match parts.next() {
        Some("down") => Arrow::Down,
        Some("left") => Arrow::Left,
        Some("right") => Arrow::Right,
        Some("up") => Arrow::Up,
        _ => bail!("line {line_number}: arrow must be down, left, right, or up"),
    };
    let count = parts
        .next()
        .map(str::parse)
        .transpose()
        .with_context(|| format!("line {line_number}: invalid arrow count"))?
        .unwrap_or(1);

    if parts.next().is_some() {
        bail!("line {line_number}: arrow directive has too many arguments");
    }

    Ok(Instruction::SendArrow {
        direction,
        count,
        enter,
    })
}

struct Recording {
    exit_status: i32,
    snapshot_labels: Vec<String>,
}

async fn record(
    options: &Expect,
    script: &Path,
    cast: &Path,
    instructions: ParsedScript,
) -> Result<Recording> {
    let mut env = HashMap::new();
    env.insert("HISTFILE".to_owned(), "/dev/null".to_owned());
    env.insert("PS1".to_owned(), "$ ".to_owned());
    env.insert("TERM".to_owned(), "xterm-256color".to_owned());
    env.insert("ASCIINEMA_EXPECT".to_owned(), "1".to_owned());

    let observer: Box<dyn ExecutionObserver> = if options.monitor {
        Box::new(Monitor::start()?)
    } else {
        Box::new(NullObserver)
    };
    let tty_size = observer
        .pty_size()
        .unwrap_or(TtySize(options.cols, options.rows));
    let shell = options.shell.to_string_lossy().to_string();
    let command = [shell, "--noprofile".to_owned(), "--norc".to_owned()];
    let working_dir = script
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let pty = pty::spawn_in_dir(&command, tty_size.into(), &env, Some(working_dir))?;
    let header = Header {
        term_cols: tty_size.0,
        term_rows: tty_size.1,
        term_type: Some("xterm-256color".to_owned()),
        timestamp: Some(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()),
        command: Some(command.join(" ")),
        env: Some(env),
        ..Default::default()
    };
    let writer = CastWriter::new(cast, header)?;
    let progress_total = instructions.instructions.len();
    let mut recorder = Recorder {
        pty,
        writer,
        epoch: Instant::now(),
        expect_buffer: String::new(),
        output_decoder: Utf8Decoder::new(),
        timeout: Duration::from_secs(options.timeout),
        delay: Duration::from_millis(options.delay),
        wait: Duration::from_millis(options.wait),
        observer,
        progress_total,
        progress_index: 0,
        snapshot_labels: Vec::new(),
    };

    for SourceInstruction { line, instruction } in instructions.instructions {
        sleep(recorder.wait).await;
        recorder.execute(line, instruction).await?;
    }

    recorder.finish().await
}

struct CastWriter {
    encoder: V3Encoder,
    writer: BufWriter<File>,
}

impl CastWriter {
    fn new(path: &Path, header: Header) -> Result<Self> {
        let file = File::create(path)
            .with_context(|| format!("can't create cast {}", path.to_string_lossy()))?;
        let mut encoder = V3Encoder::new();
        let mut writer = BufWriter::new(file);
        writer.write_all(&encoder.header(&header))?;

        Ok(Self { encoder, writer })
    }

    fn write_event(&mut self, event: Event) -> Result<()> {
        self.writer.write_all(&self.encoder.event(&event))?;
        self.writer.flush()?;
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

struct Recorder {
    pty: Pty,
    writer: CastWriter,
    epoch: Instant,
    expect_buffer: String,
    output_decoder: Utf8Decoder,
    timeout: Duration,
    delay: Duration,
    wait: Duration,
    observer: Box<dyn ExecutionObserver>,
    progress_total: usize,
    progress_index: usize,
    snapshot_labels: Vec<String>,
}

impl Recorder {
    async fn execute(&mut self, line: usize, instruction: Instruction) -> Result<()> {
        self.show_progress(line, &instruction)?;

        match instruction {
            Instruction::Command(command) => {
                self.send_text(&command).await?;
                self.send_bytes(b"\n").await?;
            }
            Instruction::Delay(delay) => self.delay = delay,
            Instruction::Expect(pattern) => self.expect(&pattern).await?,
            Instruction::Snapshot(label) => {
                self.writer
                    .write_event(Event::marker(self.elapsed(), label.clone()))?;
                self.snapshot_labels.push(label);
            }
            Instruction::Send(text) => self.send_text(&text).await?,
            Instruction::SendCharacter(text) => self.send_bytes(text.as_bytes()).await?,
            Instruction::SendArrow {
                direction,
                count,
                enter,
            } => {
                for _ in 0..count {
                    sleep(self.delay).await;
                    self.send_bytes(direction.bytes()).await?;
                    if enter {
                        self.send_bytes(b"\n").await?;
                    }
                }
            }
            Instruction::SendControl(control) => self.send_control(control).await?,
            Instruction::Wait(wait) => self.wait = wait,
        }

        Ok(())
    }

    async fn send_text(&mut self, text: &str) -> Result<()> {
        for character in text.chars() {
            self.send_bytes(character.to_string().as_bytes()).await?;
            sleep(self.delay).await;
        }

        Ok(())
    }

    async fn send_control(&mut self, control: char) -> Result<()> {
        let control = control.to_ascii_lowercase();
        if !control.is_ascii_alphabetic() {
            bail!("sendcontrol expects an ASCII letter");
        }

        self.send_bytes(&[control as u8 - b'a' + 1]).await
    }

    async fn send_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let written = self.pty.write(remaining).await?;
            if written == 0 {
                bail!("PTY stopped accepting input");
            }
            remaining = &remaining[written..];
        }

        let input = String::from_utf8_lossy(bytes).into_owned();
        self.writer
            .write_event(Event::input(self.elapsed(), input))?;

        Ok(())
    }

    async fn expect(&mut self, pattern: &str) -> Result<()> {
        let pattern = Regex::new(pattern)?;
        let deadline = tokio::time::Instant::now() + self.timeout;

        loop {
            if let Some(found) = pattern.find(&self.expect_buffer) {
                self.expect_buffer.drain(..found.end());
                return Ok(());
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for pattern {pattern:?}");
            }

            let received = timeout(remaining, self.receive_output())
                .await
                .map_err(|_| anyhow!("timed out waiting for pattern {pattern:?}"))??;
            if !received {
                bail!("shell exited while waiting for pattern {pattern:?}");
            }
        }
    }

    async fn receive_output(&mut self) -> Result<bool> {
        let mut bytes = [0u8; 8192];
        let count = self.pty.read(&mut bytes).await?;
        if count == 0 {
            return Ok(false);
        }

        self.observer.output(&bytes[..count])?;

        let output = self.output_decoder.feed(&bytes[..count]);
        if !output.is_empty() {
            self.writer
                .write_event(Event::output(self.elapsed(), output.clone()))?;
            self.expect_buffer.push_str(&output);
            if self.expect_buffer.len() > EXPECT_BUFFER_LIMIT {
                let keep_from = self.expect_buffer.len() - EXPECT_BUFFER_LIMIT;
                self.expect_buffer.drain(..keep_from);
            }
        }

        Ok(true)
    }

    fn show_progress(&mut self, line: usize, instruction: &Instruction) -> Result<()> {
        self.progress_index += 1;
        let (label, summary) = instruction_summary(instruction);
        self.observer.instruction(Progress {
            current: self.progress_index,
            total: self.progress_total,
            line,
            label,
            summary: &summary,
        })?;

        Ok(())
    }

    async fn finish(mut self) -> Result<Recording> {
        self.settle_output().await?;
        self.send_bytes(&[4]).await?;

        loop {
            match timeout(Duration::from_secs(5), self.receive_output()).await {
                Ok(Ok(true)) => continue,
                Ok(Ok(false)) => break,
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    self.pty.kill();
                    break;
                }
            }
        }

        let status = match self.pty.wait(None).await? {
            WaitStatus::Exited(_, status) => status,
            WaitStatus::Signaled(_, signal, _) => 128 + signal as i32,
            _ => 1,
        };
        self.writer
            .write_event(Event::exit(self.elapsed(), status))?;
        self.observer.finish()?;
        self.writer.finish()?;

        Ok(Recording {
            exit_status: status,
            snapshot_labels: self.snapshot_labels,
        })
    }

    async fn settle_output(&mut self) -> Result<()> {
        // A preceding `expect` can have matched output that arrived before the
        // last command completed. Give the shell a short quiet window before
        // sending EOF so it is received at a fresh prompt.
        loop {
            match timeout(Duration::from_millis(50), self.receive_output()).await {
                Ok(Ok(true)) => continue,
                Ok(Ok(false)) => return Ok(()),
                Ok(Err(error)) => return Err(error),
                Err(_) => return Ok(()),
            }
        }
    }

    fn elapsed(&self) -> Duration {
        self.epoch.elapsed()
    }
}

fn instruction_summary(instruction: &Instruction) -> (&'static str, String) {
    match instruction {
        Instruction::Command(command) => ("command", command.replace('\n', " ")),
        Instruction::Delay(delay) => ("delay", format!("{} ms", delay.as_millis())),
        Instruction::Expect(pattern) => ("expect", pattern.clone()),
        Instruction::Snapshot(label) => ("snapshot", label.clone()),
        Instruction::Send(text) => ("send", format!("{} character(s)", text.chars().count())),
        Instruction::SendCharacter(text) => (
            "sendcharacter",
            format!("{} character(s)", text.chars().count()),
        ),
        Instruction::SendArrow {
            direction,
            count,
            enter,
        } => {
            let direction = match direction {
                Arrow::Down => "down",
                Arrow::Left => "left",
                Arrow::Right => "right",
                Arrow::Up => "up",
            };
            let action = if *enter { "sendlinearrow" } else { "sendarrow" };
            (action, format!("{direction} x{count}"))
        }
        Instruction::SendControl(control) => ("sendcontrol", control.to_string()),
        Instruction::Wait(wait) => ("wait", format!("{} ms", wait.as_millis())),
    }
}

fn parse_theme(theme: &str) -> Result<agg::Theme> {
    agg::Theme::from_str(theme, true).map_err(|_| anyhow!("unknown agg theme {theme:?}"))
}

fn render_gif(
    cast: &Path,
    gif: &Path,
    theme: agg::Theme,
    font_size: usize,
    selection: Option<agg::SelectionSpec>,
) -> Result<()> {
    let input = BufReader::new(File::open(cast)?);
    let output = BufWriter::new(File::create(gif)?);
    let config = agg::Config {
        font_size,
        selection: selection.unwrap_or_default(),
        show_progress_bar: false,
        theme: Some(theme),
        ..Default::default()
    };

    match agg::run(input, output, config) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(gif);
            Err(error)
        }
    }
}

fn render_snapshot_pngs(
    paths: &BuildPaths,
    theme: agg::Theme,
    font_size: usize,
    ffmpeg: &Path,
    labels: &[String],
) -> Result<()> {
    fs::create_dir_all(&paths.snapshot_dir)?;

    status::highlight!("Rendering {} snapshot(s) from markers", labels.len());
    let marker_gif = paths
        .output_dir
        .join(format!(".{}-snapshots.gif", paths.stem));
    let temporary_pngs = (1..=labels.len())
        .map(|index| {
            paths
                .output_dir
                .join(format!(".{}-snapshot-{index:02}.png", paths.stem))
        })
        .collect::<Vec<_>>();
    let temporary_pattern = paths
        .output_dir
        .join(format!(".{}-snapshot-%02d.png", paths.stem));

    let result = (|| {
        let selection = "markers"
            .parse::<agg::SelectionSpec>()
            .map_err(|error| anyhow!(error))?;
        render_gif(&paths.cast, &marker_gif, theme, font_size, Some(selection))?;
        run_ffmpeg(
            ffmpeg,
            vec![
                "-y".into(),
                "-i".into(),
                marker_gif.as_os_str().to_owned(),
                "-vf".into(),
                format!("fps=1/{}", agg::DEFAULT_LAST_FRAME_DURATION).into(),
                "-frames:v".into(),
                labels.len().to_string().into(),
                temporary_pattern.into_os_string(),
            ],
        )?;

        for ((index, label), temporary_png) in labels.iter().enumerate().zip(&temporary_pngs) {
            let destination = paths
                .snapshot_dir
                .join(format!("{}.png", marker_name(label, index + 1)));
            fs::rename(temporary_png, destination)?;
        }

        Ok(())
    })();

    let _ = fs::remove_file(&marker_gif);
    for temporary_png in &temporary_pngs {
        let _ = fs::remove_file(temporary_png);
    }

    result
}

fn encode_mp4(ffmpeg: &Path, gif: &Path, mp4: &Path) -> Result<()> {
    run_ffmpeg(
        ffmpeg,
        vec![
            "-y".into(),
            "-i".into(),
            gif.as_os_str().to_owned(),
            "-an".into(),
            "-movflags".into(),
            "faststart".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-vf".into(),
            "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
            "-c:v".into(),
            "libx264".into(),
            mp4.as_os_str().to_owned(),
        ],
    )
}

fn run_ffmpeg(ffmpeg: &Path, args: Vec<OsString>) -> Result<()> {
    let status = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
        .with_context(|| format!("can't execute ffmpeg at {}", ffmpeg.to_string_lossy()))?;

    if status.success() {
        Ok(())
    } else {
        bail!("ffmpeg exited with {status}")
    }
}

fn marker_name(label: &str, index: usize) -> String {
    let normalized = label
        .chars()
        .map(|character| match character {
            character if character.is_ascii_alphanumeric() => character,
            '-' | '_' => character,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();

    if normalized.is_empty() {
        format!("marker-{index}")
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_commands_directives_and_markers() {
        let instructions = parse_script_content(
            "#$ expect \\$\n\
             echo hello\n\
             #$ snapshot greeting\n\
             #$ sendcontrol r\n\
             #$ sendarrow up 2\n",
        )
        .unwrap();

        assert_eq!(
            instructions,
            vec![
                Instruction::Expect("\\$".to_owned()),
                Instruction::Command("echo hello".to_owned()),
                Instruction::Snapshot("greeting".to_owned()),
                Instruction::SendControl('r'),
                Instruction::SendArrow {
                    direction: Arrow::Up,
                    count: 2,
                    enter: false,
                },
            ]
        );
    }

    #[test]
    fn parses_heredoc_body_without_treating_source_as_script_comments() {
        let instructions = parse_script_content(
            r#"cat > hello.c <<'EOF'
#include <stdio.h>
int main(void) { return 0; }
EOF
#$ snapshot source
"#,
        )
        .unwrap();

        assert_eq!(
            instructions,
            vec![
                Instruction::Command("cat > hello.c <<'EOF'".to_owned()),
                Instruction::Send(
                    "#include <stdio.h>\nint main(void) { return 0; }\nEOF\n".to_owned()
                ),
                Instruction::Snapshot("source".to_owned()),
            ]
        );
    }

    #[test]
    fn records_source_lines_for_progress() {
        let parsed = parse_script_with_lines(
            r#"#$ snapshot source
cat > hello.c <<'EOF'
#include <stdio.h>
EOF
echo done
"#,
        )
        .unwrap();

        let lines = parsed
            .instructions
            .iter()
            .map(|instruction| instruction.line)
            .collect::<Vec<_>>();

        assert_eq!(lines, vec![1, 2, 2, 5]);
    }

    #[test]
    fn decodes_legacy_directive_escapes_without_breaking_regex_escapes() {
        let instructions =
            parse_script_content("#$ send hello\\nworld\n#$ expect done\\r\\n\\$\n").unwrap();

        assert_eq!(
            instructions,
            vec![
                Instruction::Send("hello\nworld".to_owned()),
                Instruction::Expect("done\r\n\\$".to_owned()),
            ]
        );
    }

    #[test]
    fn build_paths_use_script_stem_by_default() {
        let paths = BuildPaths::new(Path::new("examples/demo.sh"), None).unwrap();

        assert_eq!(paths.output_dir, PathBuf::from("examples/demo"));
        assert_eq!(paths.cast, PathBuf::from("examples/demo/demo.cast"));
        assert_eq!(paths.snapshot_dir, PathBuf::from("examples/demo/snapshots"));
    }

    #[test]
    fn scenario_requires_shell_extension() {
        assert!(BuildPaths::new(Path::new("demo"), None).is_err());
        assert!(BuildPaths::new(Path::new("demo.txt"), None).is_err());
    }

    #[test]
    fn marker_names_are_safe_and_stable() {
        assert_eq!(marker_name("Build #1", 1), "Build--1");
        assert_eq!(marker_name("", 2), "marker-2");
    }

    #[test]
    fn cli_accepts_explicit_shell_script() {
        let monitor =
            crate::cli::Cli::try_parse_from(["asciinema", "expect", "--monitor", "demo.sh"])
                .unwrap();
        assert!(matches!(
            monitor.command,
            crate::cli::Commands::Expect(crate::cli::Expect { monitor: true, .. })
        ));
    }

    #[test]
    fn build_writes_cast_and_all_media_artifacts() {
        if which::which("ffmpeg").is_err() {
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("demo.sh");
        fs::write(
            &script,
            "#$ expect \\$\n\
             echo expect\n\
             #$ expect expect\n\
             #$ snapshot greeting\n\
             echo second marker\n\
             #$ expect second marker\n\
             #$ snapshot second\n\
             touch relative-path-check\n\
             #$ expect \\$\n",
        )
        .unwrap();

        let command = Expect {
            script: script.clone(),
            output_dir: None,
            shell: PathBuf::from("/bin/bash"),
            timeout: 10,
            delay: 1,
            wait: 1,
            monitor: false,
            cols: 80,
            rows: 24,
            theme: "github-light".to_owned(),
            font_size: 12,
            ffmpeg: PathBuf::from("ffmpeg"),
        };
        assert_eq!(command.run().unwrap(), ExitCode::SUCCESS);

        let output = directory.path().join("demo");
        let cast = output.join("demo.cast");
        let parsed = crate::asciicast::open_from_path(&cast).unwrap();
        let markers = parsed
            .events
            .filter_map(Result::ok)
            .filter(|event| matches!(event.data, crate::asciicast::EventData::Marker(_)))
            .count();

        assert_eq!(markers, 2);
        assert!(output.join("demo.gif").is_file());
        assert!(output.join("demo.mp4").is_file());
        assert!(output.join("snapshots/greeting.png").is_file());
        assert!(output.join("snapshots/second.png").is_file());
        assert!(directory.path().join("relative-path-check").is_file());
    }

    #[test]
    fn gcc_example_compiles_runs_and_writes_all_marker_snapshots() {
        if which::which("ffmpeg").is_err() || which::which("gcc").is_err() {
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("gcc-hello.sh");
        fs::write(&script, include_str!("../examples/gcc-hello.sh")).unwrap();

        let command = Expect {
            script: script.clone(),
            output_dir: None,
            shell: PathBuf::from("/bin/bash"),
            timeout: 10,
            delay: 1,
            wait: 20,
            monitor: false,
            cols: 80,
            rows: 24,
            theme: "github-light".to_owned(),
            font_size: 12,
            ffmpeg: PathBuf::from("ffmpeg"),
        };
        assert_eq!(command.run().unwrap(), ExitCode::SUCCESS);

        let output = directory.path().join("gcc-hello");
        let cast = fs::read_to_string(output.join("gcc-hello.cast")).unwrap();
        let parsed = crate::asciicast::open_from_path(output.join("gcc-hello.cast")).unwrap();
        let events = parsed.events.filter_map(Result::ok).collect::<Vec<_>>();
        let marker_count = events
            .iter()
            .filter(|event| matches!(event.data, crate::asciicast::EventData::Marker(_)))
            .count();
        let snapshots = [
            "before-create-source.png",
            "after-create-source.png",
            "before-compile.png",
            "after-compile.png",
            "before-run.png",
            "after-run.png",
        ];

        assert!(directory.path().join("hello.c").is_file());
        assert!(directory.path().join("hello").is_file());
        assert!(cast.contains("Hello, expect!"));
        assert_eq!(marker_count, snapshots.len());
        let marker_index = |label: &str| {
            events
                .iter()
                .position(|event| {
                    matches!(&event.data, crate::asciicast::EventData::Marker(event_label) if event_label == label)
                })
                .unwrap()
        };
        let output_index = |needle: &str| {
            events
                .iter()
                .position(|event| {
                    matches!(&event.data, crate::asciicast::EventData::Output(output) if output.contains(needle))
                })
                .unwrap()
        };
        let gcc_output = output_index("gcc -Wall -Wextra -std=c11 hello.c -o hello");
        let hello_output = output_index("Hello, expect!");
        let gcc_prompt = events
            .iter()
            .enumerate()
            .skip(gcc_output + 1)
            .find_map(|(index, event)| {
                matches!(&event.data, crate::asciicast::EventData::Output(output) if output.contains("$ "))
                    .then_some(index)
            })
            .unwrap();

        assert!(marker_index("before-compile") < gcc_output);
        assert!(marker_index("after-compile") > gcc_prompt);
        assert!(marker_index("before-run") > gcc_prompt);
        assert!(marker_index("after-run") > hello_output);
        for snapshot in snapshots {
            assert!(output.join("snapshots").join(snapshot).is_file());
        }
        let snapshots = output.join("snapshots");
        assert_eq!(
            fs::read(snapshots.join("after-create-source.png")).unwrap(),
            fs::read(snapshots.join("before-compile.png")).unwrap()
        );
        assert_eq!(
            fs::read(snapshots.join("after-compile.png")).unwrap(),
            fs::read(snapshots.join("before-run.png")).unwrap()
        );
    }
}
