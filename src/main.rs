mod app;
mod clipboard;
mod model;
mod origin;
mod screen;
mod selection;
mod text;
mod theme;
mod ui;
mod worker;

use std::io::{self, IsTerminal, Stdout, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Effect, Job, Msg, ToastKind};
use crate::clipboard::CopyOutcome;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn main() -> anyhow::Result<()> {
    if !io::stdout().is_terminal() {
        eprintln!("ot needs an interactive terminal");
        std::process::exit(1);
    }

    enable_raw_mode()?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
    let _guard = Restore;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        Hide
    )?;

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = run(&mut terminal);
    drop(terminal);
    result
}

fn run(terminal: &mut Term) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    let (mut app, jobs) = App::boot();
    spawn_jobs(&tx, app.origin.clone(), jobs);

    loop {
        let mut drawn = screen::Screen::default();
        terminal.draw(|frame| {
            drawn = ui::draw(frame, &app);
        })?;
        app.adopt(drawn);

        if crossterm::event::poll(app.poll_timeout())? && handle_event(&mut app, terminal, &tx)? {
            break;
        }
        while let Ok(msg) = rx.try_recv() {
            let jobs = app.on_msg(msg);
            spawn_jobs(&tx, app.origin.clone(), jobs);
        }
        app.tick();
    }
    Ok(())
}

fn handle_event(app: &mut App, terminal: &mut Term, tx: &Sender<Msg>) -> anyhow::Result<bool> {
    let quit = match crossterm::event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Release => false,
        Event::Key(key) => {
            let effect = app.on_key(key);
            apply_effect(app, effect, tx)
        }
        Event::Mouse(mouse) => {
            let effect = app.on_mouse(mouse, terminal.current_buffer_mut());
            apply_effect(app, effect, tx)
        }
        Event::Paste(text) => {
            let effect = app.on_paste(text);
            apply_effect(app, effect, tx)
        }
        Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => false,
    };
    Ok(quit)
}

fn apply_effect(app: &mut App, effect: Effect, tx: &Sender<Msg>) -> bool {
    match effect {
        Effect::None => false,
        Effect::Quit => true,
        Effect::Copy { text, toast } => {
            apply_copy(app, &text, &toast);
            false
        }
        Effect::Jobs(jobs) => {
            spawn_jobs(tx, app.origin.clone(), jobs);
            false
        }
    }
}

fn apply_copy(app: &mut App, text: &str, toast: &str) {
    match clipboard::copy(text) {
        Ok(CopyOutcome::Native { .. }) => app.toast(toast, ToastKind::Info),
        Ok(CopyOutcome::Unverified { backup }) => {
            let message = match backup {
                Some(path) => format!("{toast} · saved {}", clipboard::display_path(&path)),
                None => format!("{toast} · clipboard unverified"),
            };
            app.toast(message, ToastKind::Info);
        }
        Ok(CopyOutcome::BackupOnly { backup }) => app.toast(
            format!(
                "Clipboard unavailable. Saved {}",
                clipboard::display_path(&backup)
            ),
            ToastKind::Err,
        ),
        Err(err) => app.toast(err, ToastKind::Err),
    }
}

fn spawn_jobs(tx: &Sender<Msg>, bin: Option<PathBuf>, jobs: Vec<Job>) {
    let Some(bin) = bin else {
        return;
    };
    for job in jobs {
        let tx = tx.clone();
        let bin = bin.clone();
        thread::spawn(move || {
            let _ = tx.send(worker::execute(&bin, job));
        });
    }
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = out.flush();
}
