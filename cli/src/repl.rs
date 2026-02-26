use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustyline::completion::Completer;
use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{
    Cmd as ReadlineCmd, CompletionType, ConditionalEventHandler, Editor, Event, EventContext,
    EventHandler, Helper, KeyCode, KeyEvent, Modifiers, RepeatCount,
};
use termimad::MadSkin;
use tokio::sync::mpsc;

const SLASH_COMMANDS: &[&str] = &["/help", "/quit", "/exit"];

struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }
        let prefix = &line[..pos];
        let matches: Vec<String> = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| cmd.to_string())
            .collect();
        Ok((0, matches))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if !line.starts_with('/') || pos < line.len() {
            return None;
        }
        SLASH_COMMANDS
            .iter()
            .find(|cmd| cmd.starts_with(line) && **cmd != line)
            .map(|cmd| cmd[line.len()..].to_string())
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}

impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

struct EscInterruptHandler {
    triggered: Arc<AtomicBool>,
}

impl ConditionalEventHandler for EscInterruptHandler {
    fn handle(
        &self,
        _evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        _ctx: &EventContext,
    ) -> Option<ReadlineCmd> {
        self.triggered.store(true, Ordering::Relaxed);
        Some(ReadlineCmd::Interrupt)
    }
}

fn make_skin() -> MadSkin {
    let mut skin = MadSkin::default();
    skin.set_headers_fg(termimad::crossterm::style::Color::Yellow);
    skin.bold.set_fg(termimad::crossterm::style::Color::White);
    skin.italic.set_fg(termimad::crossterm::style::Color::Magenta);
    skin.inline_code.set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block.set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block.left_margin = 2;
    skin
}

fn print_help() {
    let h = "\x1b[1m";
    let c = "\x1b[1;36m";
    let d = "\x1b[90m";
    let r = "\x1b[0m";

    println!();
    println!("  {h}Dinoe REPL{r}");
    println!();
    println!("  {h}Commands{r}");
    println!("  {c}/help{r}              {d}show this help{r}");
    println!("  {c}/quit{r} {c}/exit{r}        {d}exit the repl{r}");
    println!();
}

fn history_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".dinoe")
        .join("history")
}

pub fn print_markdown(content: &str) {
    let width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let skin = make_skin();
    let text = termimad::FmtText::from(&skin, content, Some(width));
    print!("{text}");
}

pub enum ReplCommand {
    Input(String),
    Quit,
}

pub struct ReplHandle {
    input_rx: mpsc::Receiver<ReplCommand>,
    done_tx: mpsc::Sender<()>,
}

impl ReplHandle {
    pub async fn recv(&mut self) -> Option<ReplCommand> {
        self.input_rx.recv().await
    }

    pub async fn signal_done(&self) {
        let _ = self.done_tx.send(()).await;
    }
}

pub fn start() -> ReplHandle {
    let (input_tx, input_rx) = mpsc::channel(32);
    let (done_tx, mut done_rx) = mpsc::channel::<()>(1);

    std::thread::spawn(move || {
        let config = Config::builder()
            .history_ignore_dups(true)
            .expect("valid config")
            .auto_add_history(true)
            .completion_type(CompletionType::List)
            .build();

        let mut rl = match Editor::with_config(config) {
            Ok(editor) => editor,
            Err(e) => {
                eprintln!("Failed to initialize line editor: {e}");
                return;
            }
        };

        rl.set_helper(Some(ReplHelper));

        let esc_triggered = Arc::new(AtomicBool::new(false));
        rl.bind_sequence(
            KeyEvent(KeyCode::Esc, Modifiers::NONE),
            EventHandler::Conditional(Box::new(EscInterruptHandler {
                triggered: Arc::clone(&esc_triggered),
            })),
        );

        let hist_path = history_path();
        if let Some(parent) = hist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.load_history(&hist_path);

        println!("\x1b[1mDinoe\x1b[0m  /help for commands, /quit to exit");
        println!();

        loop {
            match rl.readline("\x1b[1;36m\u{203A}\x1b[0m ") {
                Ok(line) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    match line.to_lowercase().as_str() {
                        "/quit" | "/exit" => {
                            let _ = input_tx.blocking_send(ReplCommand::Quit);
                            break;
                        }
                        "/help" => {
                            print_help();
                            continue;
                        }
                        _ => {}
                    }

                    if input_tx.blocking_send(ReplCommand::Input(line.to_string())).is_err() {
                        break;
                    }

                    let _ = done_rx.blocking_recv();
                }
                Err(ReadlineError::Interrupted) => {
                    if esc_triggered.swap(false, Ordering::Relaxed) {
                        println!("\x1b[90mInterrupted\x1b[0m");
                    } else {
                        let _ = input_tx.blocking_send(ReplCommand::Quit);
                        break;
                    }
                }
                Err(ReadlineError::Eof) => {
                    let _ = input_tx.blocking_send(ReplCommand::Quit);
                    break;
                }
                Err(e) => {
                    eprintln!("Input error: {e}");
                    break;
                }
            }
        }

        let _ = rl.save_history(&history_path());
    });

    ReplHandle {
        input_rx,
        done_tx,
    }
}
