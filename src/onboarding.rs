//! HardClaw Onboarding TUI
//!
//! Interactive terminal application for:
//! - Creating/loading wallets
//! - Checking the verification environment
//! - Running a verifier node

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};

use hardclaw::{
    verifier::{AIModelCheck, EnvironmentCheck},
    wallet::Wallet,
};

/// Application state
enum AppState {
    Welcome,
    MainMenu,
    CreateWallet,
    WalletCreated {
        address: String,
        path: String,
        seed_phrase: String,
    },
    LoadWallet,
    WalletLoaded {
        address: String,
    },
    /// Selection screen - user chooses which environments to set up
    EnvironmentSelection {
        runtime_checks: Vec<EnvironmentCheck>,
        ai_check: AIModelCheck,
    },
    /// Running setup for selected environments
    #[allow(dead_code)] // UI state rendered during setup
    EnvironmentSetup,
    /// Results after setup completes
    EnvironmentChecked {
        runtime_checks: Vec<EnvironmentCheck>,
        ai_check: AIModelCheck,
    },
    RunNode,
    #[allow(dead_code)] // Planned feature - node integration
    NodeRunning,
    Help,
    Quit,
}

/// Menu selection state
struct MenuState {
    items: Vec<&'static str>,
    selected: usize,
}

impl MenuState {
    fn new(items: Vec<&'static str>) -> Self {
        Self { items, selected: 0 }
    }

    fn next(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    fn previous(&mut self) {
        self.selected = self.selected.checked_sub(1).unwrap_or(self.items.len() - 1);
    }
}

/// Environment selection for setup
struct EnvSelectionState {
    /// Python standalone installation
    python_selected: bool,
    python_detected: bool,
    python_version: Option<String>,
    /// AI models setup (Ollama + llama)
    ai_selected: bool,
    ai_detected: bool,
    ai_models: Vec<String>,
    /// Current cursor position (0=Python, 1=AI, 2=Run Setup, 3=Skip)
    cursor: usize,
}

impl EnvSelectionState {
    fn new(runtime_checks: &[EnvironmentCheck], ai_check: &AIModelCheck) -> Self {
        let python_check = runtime_checks
            .iter()
            .find(|c| c.language == hardclaw::verifier::LanguageSupport::Python);

        let python_detected = python_check.map(|c| c.available).unwrap_or(false);
        let python_version = python_check.and_then(|c| c.version.clone());

        Self {
            // Nothing pre-selected - user explicitly chooses what to set up
            // They can use existing installations (Ollama, system Python, etc.)
            python_selected: false,
            python_detected,
            python_version,
            ai_selected: false,
            ai_detected: ai_check.available,
            ai_models: ai_check.models.clone(),
            cursor: 0,
        }
    }

    fn next(&mut self) {
        self.cursor = (self.cursor + 1) % 4;
    }

    fn previous(&mut self) {
        self.cursor = self.cursor.checked_sub(1).unwrap_or(3);
    }

    fn toggle(&mut self) {
        match self.cursor {
            0 => self.python_selected = !self.python_selected,
            1 => self.ai_selected = !self.ai_selected,
            _ => {}
        }
    }

    fn any_selected(&self) -> bool {
        self.python_selected || self.ai_selected
    }
}

/// Main application
struct App {
    state: AppState,
    menu: MenuState,
    wallet: Option<Wallet>,
    message: Option<String>,
    env_selection: Option<EnvSelectionState>,
}

impl App {
    fn new() -> Self {
        let menu_items = if Wallet::default_exists() {
            vec![
                "Load Wallet",
                "Create New Wallet",
                "Check Verification Environment",
                "Run Verifier Node",
                "Help",
                "Quit",
            ]
        } else {
            vec![
                "Create Wallet",
                "Load Wallet",
                "Check Verification Environment",
                "Run Verifier Node",
                "Help",
                "Quit",
            ]
        };

        Self {
            state: AppState::Welcome,
            menu: MenuState::new(menu_items),
            wallet: None,
            message: None,
            env_selection: None,
        }
    }

    fn handle_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        // Ctrl+C or 'q' always quits (except during active setup)
        if (modifiers.contains(KeyModifiers::CONTROL) && key == KeyCode::Char('c'))
            || (key == KeyCode::Char('q') && !matches!(self.state, AppState::EnvironmentSetup))
        {
            return true;
        }

        match &self.state {
            AppState::Welcome => {
                self.state = AppState::MainMenu;
            }
            AppState::MainMenu => match key {
                KeyCode::Up | KeyCode::Char('k') => self.menu.previous(),
                KeyCode::Down | KeyCode::Char('j') => self.menu.next(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.handle_menu_selection();
                }
                _ => {}
            },
            AppState::CreateWallet => match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.create_wallet();
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::WalletCreated { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::LoadWallet => match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.load_wallet();
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::WalletLoaded { .. } => {
                self.state = AppState::MainMenu;
            }
            AppState::EnvironmentSelection { .. } => {
                if let Some(ref mut selection) = self.env_selection {
                    match key {
                        KeyCode::Up | KeyCode::Char('k') => selection.previous(),
                        KeyCode::Down | KeyCode::Char('j') => selection.next(),
                        KeyCode::Char(' ') => selection.toggle(),
                        KeyCode::Enter => {
                            if selection.cursor == 2 && selection.any_selected() {
                                // Run Setup selected - signal to run (terminal will be dropped first)
                                self.state = AppState::EnvironmentSetup;
                            } else if selection.cursor == 3 || !selection.any_selected() {
                                // Skip or nothing selected
                                self.state = AppState::MainMenu;
                                self.env_selection = None;
                            }
                        }
                        KeyCode::Esc => {
                            self.state = AppState::MainMenu;
                            self.env_selection = None;
                        }
                        _ => {}
                    }
                }
            }
            AppState::EnvironmentSetup => {
                // Shouldn't receive input here - setup is running
            }
            AppState::EnvironmentChecked { .. } => {
                self.state = AppState::MainMenu;
                self.env_selection = None;
            }
            AppState::RunNode => {
                self.state = AppState::MainMenu;
            }
            AppState::Help => {
                self.state = AppState::MainMenu;
            }
            AppState::NodeRunning => match key {
                KeyCode::Esc => {
                    self.state = AppState::MainMenu;
                }
                _ => {}
            },
            AppState::Quit => return true,
        }

        false
    }

    fn handle_menu_selection(&mut self) {
        let selected = self.menu.items[self.menu.selected];
        match selected {
            "Create Wallet" | "Create New Wallet" => {
                self.state = AppState::CreateWallet;
            }
            "Load Wallet" => {
                self.state = AppState::LoadWallet;
            }
            "Check Verification Environment" => {
                self.check_environment();
            }
            "Run Verifier Node" => {
                if self.wallet.is_none() {
                    self.message = Some("Please create or load a wallet first".to_string());
                } else {
                    self.state = AppState::RunNode;
                }
            }
            "Help" => {
                self.state = AppState::Help;
            }
            "Quit" => {
                self.state = AppState::Quit;
            }
            _ => {}
        }
    }

    fn create_wallet(&mut self) {
        // Generate wallet with BIP39 mnemonic
        let mnemonic = hardclaw::generate_mnemonic();
        let seed_phrase = mnemonic.to_string();
        let keypair = hardclaw::keypair_from_mnemonic(&mnemonic, "");

        // Create wallet from the keypair
        let mut wallet = Wallet::from_keypair(keypair);
        let address = wallet.address().to_string();

        match wallet.save_as_default() {
            Ok(()) => {
                let path = Wallet::default_path().display().to_string();
                self.wallet = Some(wallet);
                self.message = None;
                self.state = AppState::WalletCreated {
                    address,
                    path,
                    seed_phrase,
                };
                // Update menu to show "Load Wallet" as first option now
                self.menu = MenuState::new(vec![
                    "Load Wallet",
                    "Create New Wallet",
                    "Check Verification Environment",
                    "Run Verifier Node",
                    "Help",
                    "Quit",
                ]);
            }
            Err(e) => {
                self.message = Some(format!("Failed to save wallet: {}", e));
                self.state = AppState::MainMenu;
            }
        }
    }

    fn load_wallet(&mut self) {
        match Wallet::load_default() {
            Ok(wallet) => {
                let address = wallet.address().to_string();
                self.wallet = Some(wallet);
                self.message = None;
                self.state = AppState::WalletLoaded { address };
            }
            Err(e) => {
                self.message = Some(format!("Failed to load wallet: {}", e));
                self.state = AppState::MainMenu;
            }
        }
    }

    fn check_environment(&mut self) {
        // Detect what's already installed (read-only, no output)
        let runtime_checks = EnvironmentCheck::detect_all();
        let ai_check = AIModelCheck::detect();

        // Create selection state based on what's detected
        self.env_selection = Some(EnvSelectionState::new(&runtime_checks, &ai_check));

        // Show selection screen
        self.state = AppState::EnvironmentSelection {
            runtime_checks,
            ai_check,
        };
    }

    /// Returns true if setup should be run (terminal needs to be dropped first)
    fn should_run_setup(&self) -> bool {
        if let Some(ref selection) = self.env_selection {
            matches!(self.state, AppState::EnvironmentSetup) && selection.any_selected()
        } else {
            false
        }
    }

    /// Run the actual environment setup (call after dropping terminal)
    fn run_environment_setup_inner(&mut self) {
        // Run setup only for selected items
        if let Some(ref selection) = self.env_selection {
            let mut runtime_checks = Vec::new();

            // Setup Python if selected
            if selection.python_selected {
                runtime_checks.push(EnvironmentCheck::setup_python());
            } else {
                runtime_checks.push(EnvironmentCheck::detect_python());
            }

            // Always detect JS/TS/WASM (embedded, no setup needed)
            runtime_checks.push(EnvironmentCheck::detect_javascript());
            runtime_checks.push(EnvironmentCheck::detect_typescript());
            runtime_checks.push(EnvironmentCheck::detect_wasm());

            // Setup AI if selected
            let ai_check = if selection.ai_selected {
                AIModelCheck::setup(&["llama3.2"])
            } else {
                AIModelCheck::detect()
            };

            self.state = AppState::EnvironmentChecked {
                runtime_checks,
                ai_check,
            };
        }
    }

    fn ui(&self, frame: &mut Frame) {
        let size = frame.area();

        // Minimum terminal size for a usable TUI
        const MIN_COLS: u16 = 80;
        const MIN_ROWS: u16 = 24;
        if size.width < MIN_COLS || size.height < MIN_ROWS {
            let msg = Paragraph::new(format!(
                "Terminal too small ({}\u{00d7}{}).\nPlease resize to at least {}\u{00d7}{}.",
                size.width, size.height, MIN_COLS, MIN_ROWS
            ))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow));
            frame.render_widget(msg, size);
            return;
        }

        // Main layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Footer
            ])
            .split(size);

        // Header
        self.render_header(frame, chunks[0]);

        // Content based on state
        match &self.state {
            AppState::Welcome => self.render_welcome(frame, chunks[1]),
            AppState::MainMenu => self.render_main_menu(frame, chunks[1]),
            AppState::CreateWallet => self.render_create_wallet(frame, chunks[1]),
            AppState::WalletCreated {
                address,
                path,
                seed_phrase,
            } => {
                self.render_wallet_created(frame, chunks[1], address, path, seed_phrase);
            }
            AppState::LoadWallet => self.render_load_wallet(frame, chunks[1]),
            AppState::WalletLoaded { address } => {
                self.render_wallet_loaded(frame, chunks[1], address);
            }
            AppState::EnvironmentSelection {
                runtime_checks,
                ai_check,
            } => {
                self.render_environment_selection(frame, chunks[1], runtime_checks, ai_check);
            }
            AppState::EnvironmentSetup => self.render_environment_setup(frame, chunks[1]),
            AppState::EnvironmentChecked {
                runtime_checks,
                ai_check,
            } => {
                self.render_environment_checked(frame, chunks[1], runtime_checks, ai_check);
            }
            AppState::RunNode => self.render_run_node(frame, chunks[1]),
            AppState::Help => self.render_help(frame, chunks[1]),
            AppState::NodeRunning => self.render_node_running(frame, chunks[1]),
            AppState::Quit => {}
        }

        // Footer
        self.render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let wallet_status = if let Some(wallet) = &self.wallet {
            format!(" | Wallet: {}...", &wallet.address().to_string()[..16])
        } else {
            String::new()
        };

        let header = Paragraph::new(format!("HardClaw v{}{}", hardclaw::VERSION, wallet_status))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(header, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hint = match &self.state {
            AppState::Welcome => "Press any key to continue | Ctrl+C/q: Quit",
            AppState::MainMenu => "j/k: Navigate | Enter: Select | q/Ctrl+C: Quit",
            AppState::CreateWallet | AppState::LoadWallet => {
                "Enter/y: Confirm | Esc/n: Cancel | q/Ctrl+C: Quit"
            }
            AppState::WalletCreated { .. }
            | AppState::WalletLoaded { .. }
            | AppState::RunNode
            | AppState::EnvironmentChecked { .. }
            | AppState::Help => "Press any key to continue | q/Ctrl+C: Quit",
            AppState::EnvironmentSelection { .. } => {
                "j/k: Navigate | Space: Toggle | Enter: Confirm | Esc/q: Cancel"
            }
            AppState::EnvironmentSetup => "Setting up environment... | Ctrl+C: Force quit",
            AppState::NodeRunning => "Esc: Stop node | q/Ctrl+C: Quit",
            AppState::Quit => "",
        };

        let mut footer_text = hint.to_string();
        if let Some(msg) = &self.message {
            footer_text = format!("{} | {}", msg, hint);
        }

        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(footer, area);
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let logo = r#"

    ██╗  ██╗ █████╗ ██████╗ ██████╗  ██████╗██╗      █████╗ ██╗    ██╗
    ██║  ██║██╔══██╗██╔══██╗██╔══██╗██╔════╝██║     ██╔══██╗██║    ██║
    ███████║███████║██████╔╝██║  ██║██║     ██║     ███████║██║ █╗ ██║
    ██╔══██║██╔══██║██╔══██╗██║  ██║██║     ██║     ██╔══██║██║███╗██║
    ██║  ██║██║  ██║██║  ██║██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝
    ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝

           Proof-of-Verification for the Autonomous Agent Economy

                        "We do not trust; we verify."
"#;

        let welcome = Paragraph::new(logo)
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center)
            .block(Block::default());

        frame.render_widget(welcome, area);
    }

    fn render_main_menu(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .split(area);

        let items: Vec<ListItem> = self
            .menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let style = if i == self.menu.selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let prefix = if i == self.menu.selected { "> " } else { "  " };
                ListItem::new(format!("{}{}", prefix, item)).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Main Menu ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));

        frame.render_widget(list, chunks[1]);
    }

    fn render_create_wallet(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Create New Wallet",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This will generate a new Ed25519 keypair for your HardClaw wallet."),
            Line::from(""),
            Line::from("The wallet will be saved to:"),
            Line::from(Span::styled(
                format!("  {}", Wallet::default_path().display()),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "WARNING: Keep your wallet file secure!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from("Anyone with access to this file can control your funds."),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Create wallet? (y/n)",
                Style::default().fg(Color::Green),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 60, area));
    }

    fn render_wallet_created(
        &self,
        frame: &mut Frame,
        area: Rect,
        address: &str,
        path: &str,
        seed_phrase: &str,
    ) {
        let words: Vec<&str> = seed_phrase.split_whitespace().collect();
        let mut text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "🔐 WALLET CREATED - SAVE YOUR SEED PHRASE! 🔐",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Write down these 24 words in order and store them securely.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "This is the ONLY way to recover your wallet!",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Display seed phrase in 4 columns of 6 words
        for row in 0..6 {
            let mut line_spans = vec![];
            for col in 0..4 {
                let idx = col * 6 + row;
                if idx < words.len() {
                    line_spans.push(Span::styled(
                        format!("{:2}. {:<11} ", idx + 1, words[idx]),
                        Style::default().fg(Color::Cyan),
                    ));
                }
            }
            text.push(Line::from(line_spans));
        }

        text.extend(vec![
            Line::from(""),
            Line::from(Span::styled("Address:", Style::default().fg(Color::White))),
            Line::from(Span::styled(
                address.to_string(),
                Style::default().fg(Color::Green),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("Wallet saved to: {}", path),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default().fg(Color::DarkGray),
            )),
        ]);

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );

        frame.render_widget(paragraph, centered_rect(85, 90, area));
    }

    fn render_load_wallet(&self, frame: &mut Frame, area: Rect) {
        let default_path = Wallet::default_path();
        let exists = default_path.exists();

        let text = if exists {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Load Existing Wallet",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Found wallet at:"),
                Line::from(Span::styled(
                    format!("  {}", default_path.display()),
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "Load this wallet? (y/n)",
                    Style::default().fg(Color::Green),
                )),
            ]
        } else {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No Wallet Found",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("No wallet exists at:"),
                Line::from(Span::styled(
                    format!("  {}", default_path.display()),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from("Please create a new wallet first."),
                Line::from(""),
                Line::from(Span::styled(
                    "Press any key to go back...",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        };

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 50, area));
    }

    fn render_wallet_loaded(&self, frame: &mut Frame, area: Rect, address: &str) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Wallet Loaded Successfully!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Wallet address:"),
            Line::from(Span::styled(
                address.to_string(),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to continue...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

        frame.render_widget(paragraph, centered_rect(60, 40, area));
    }

    fn render_environment_selection(
        &self,
        frame: &mut Frame,
        area: Rect,
        runtime_checks: &[EnvironmentCheck],
        _ai_check: &AIModelCheck,
    ) {
        let selection = match &self.env_selection {
            Some(s) => s,
            None => return,
        };

        let mut text = vec![
            Line::from(Span::styled(
                "🔧 Environment Setup",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Select which environments to set up:"),
            Line::from(""),
        ];

        // Python option (selection state has the detected info)
        let _python_check = runtime_checks
            .iter()
            .find(|c| c.language == hardclaw::verifier::LanguageSupport::Python);
        let python_status = if selection.python_detected {
            format!(
                "✓ Installed: {}",
                python_status_str(selection.python_version.as_deref())
            )
        } else {
            "✗ Not installed".to_string()
        };

        let python_checkbox = if selection.python_selected {
            "[x]"
        } else {
            "[ ]"
        };
        let python_style = if selection.cursor == 0 {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        text.push(Line::from(vec![
            Span::styled(
                if selection.cursor == 0 { "> " } else { "  " },
                python_style,
            ),
            Span::styled(format!("{} ", python_checkbox), python_style),
            Span::styled("Python 3.12", python_style),
            Span::styled(
                format!("  ({})", python_status),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        if selection.cursor == 0 {
            let hint = if selection.python_detected {
                "      Already installed - select only to reinstall"
            } else {
                "      Downloads standalone Python 3.12 (~30MB) to ~/.hardclaw/python/"
            };
            text.push(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )));
        }

        text.push(Line::from(""));

        // AI option — truncate model list to avoid overflow
        let ai_status = if selection.ai_detected {
            if selection.ai_models.is_empty() {
                "✓ Ollama installed, no models".to_string()
            } else {
                let models_str = selection.ai_models.join(", ");
                if models_str.len() > 40 {
                    let count = selection.ai_models.len();
                    let truncated: String = models_str.chars().take(37).collect();
                    format!("✓ {}... ({} models)", truncated, count)
                } else {
                    format!("✓ Models: {}", models_str)
                }
            }
        } else {
            "✗ Ollama not installed".to_string()
        };

        let ai_checkbox = if selection.ai_selected { "[x]" } else { "[ ]" };
        let ai_style = if selection.cursor == 1 {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        text.push(Line::from(vec![
            Span::styled(if selection.cursor == 1 { "> " } else { "  " }, ai_style),
            Span::styled(format!("{} ", ai_checkbox), ai_style),
            Span::styled("AI Code Review (Ollama)", ai_style),
            Span::styled(
                format!("  ({})", ai_status),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        if selection.cursor == 1 {
            let hint = if selection.ai_detected && !selection.ai_models.is_empty() {
                "      Your existing models will work - select only to add llama3.2"
            } else if selection.ai_detected {
                "      Ollama installed - select to download llama3.2 (~2GB)"
            } else {
                "      Installs Ollama + llama3.2 model (~2GB download)"
            };
            text.push(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )));
        }

        text.push(Line::from(""));
        text.push(Line::from(""));

        // Embedded runtimes (always available, not selectable)
        text.push(Line::from(Span::styled(
            "Always available (embedded in binary):",
            Style::default().fg(Color::DarkGray),
        )));

        for check in runtime_checks {
            if check.language == hardclaw::verifier::LanguageSupport::JavaScript
                || check.language == hardclaw::verifier::LanguageSupport::TypeScript
                || check.language == hardclaw::verifier::LanguageSupport::Wasm
            {
                text.push(Line::from(Span::styled(
                    format!(
                        "  ✓ {} ({})",
                        check.language.display_name(),
                        check.version.as_deref().unwrap_or("embedded")
                    ),
                    Style::default().fg(Color::Green),
                )));
            }
        }

        text.push(Line::from(""));
        text.push(Line::from(""));

        // Action buttons
        let run_style = if selection.cursor == 2 {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if selection.any_selected() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let skip_style = if selection.cursor == 3 {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        text.push(Line::from(vec![
            Span::styled(if selection.cursor == 2 { "> " } else { "  " }, run_style),
            Span::styled(
                if selection.any_selected() {
                    "[ Run Setup ]"
                } else {
                    "[ Nothing selected ]"
                },
                run_style,
            ),
        ]));

        text.push(Line::from(vec![
            Span::styled(if selection.cursor == 3 { "> " } else { "  " }, skip_style),
            Span::styled("[ Skip / Return to Menu ]", skip_style),
        ]));

        let paragraph = Paragraph::new(text).alignment(Alignment::Left).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Select Environments "),
        );

        frame.render_widget(paragraph, centered_rect(90, 85, area));
    }

    fn render_run_node(&self, frame: &mut Frame, area: Rect) {
        let wallet_addr = self
            .wallet
            .as_ref()
            .map(|w| w.address().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Run Verifier Node",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("To start your verifier node, run:"),
            Line::from(""),
            Line::from(Span::styled(
                "  hardclaw node --verifier",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Your node will:"),
            Line::from(Span::styled(
                "  - Connect to the HardClaw P2P network",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  - Verify solutions and earn rewards",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  - Participate in consensus",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("Node address:"),
            Line::from(Span::styled(
                format!("  {}...", &wallet_addr[..32.min(wallet_addr.len())]),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to return...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(70, 75, area));
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "HardClaw - Quick Start Guide",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "1. Create a Wallet",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Generate a new Ed25519 keypair to identify your node."),
            Line::from("   Your wallet is your identity on the network."),
            Line::from(""),
            Line::from(Span::styled(
                "2. Run Verifier Node",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Start your node to verify solutions and earn rewards."),
            Line::from("   Use 'hardclaw node --verifier' from the command line."),
            Line::from(""),
            Line::from(Span::styled(
                "3. Participate in the Network",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Submit jobs, verify solutions, and produce blocks."),
            Line::from("   Earn HCLAW rewards for honest verification."),
            Line::from(""),
            Line::from(Span::styled(
                "Protocol Roles:",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("   Requester - Submit jobs with bounties"),
            Line::from("   Solver    - Execute work, submit solutions"),
            Line::from("   Verifier  - Verify solutions, produce blocks"),
            Line::from(""),
            Line::from(Span::styled(
                "Fee Distribution: ",
                Style::default().fg(Color::White),
            )),
            Line::from("   95% Solver | 4% Verifier | 1% Burned"),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to return...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, centered_rect(80, 85, area));
    }

    fn render_node_running(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Node Running",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Status: Active"),
            Line::from("Peers: 0"),
            Line::from("Height: 1"),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'q' to stop...",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

        frame.render_widget(paragraph, centered_rect(50, 40, area));
    }

    fn render_environment_setup(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(Span::styled(
                "🔍 Checking Verification Environment",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Please wait..."),
        ];

        let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, centered_rect(60, 30, area));
    }

    fn render_environment_checked(
        &self,
        frame: &mut Frame,
        area: Rect,
        runtime_checks: &[EnvironmentCheck],
        ai_check: &AIModelCheck,
    ) {
        let mut text = vec![
            Line::from(Span::styled(
                "🔍 Validator Environment Status",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Verification Runtimes:",
                Style::default().fg(Color::Cyan),
            )),
        ];

        for check in runtime_checks {
            let (status, color) = if check.available {
                ("✓", Color::Green)
            } else {
                ("✗", Color::Red)
            };

            text.push(Line::from(vec![
                Span::styled(format!("[{}] ", status), Style::default().fg(color)),
                Span::styled(
                    check.language.display_name(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
                Span::raw(check.version.as_deref().unwrap_or("not available")),
            ]));

            // Show warnings
            for warning in &check.warnings {
                text.push(Line::from(Span::styled(
                    format!("    ⚠ {}", warning),
                    Style::default().fg(Color::Yellow),
                )));
            }

            // Show setup instructions if not available
            if let Some(instructions) = &check.setup_instructions {
                text.push(Line::from(""));
                for line in instructions.lines() {
                    text.push(Line::from(Span::styled(
                        format!("    {}", line),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
        }

        text.push(Line::from(""));
        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            "AI Safety Review:",
            Style::default().fg(Color::Cyan),
        )));

        // Ollama status
        let (ollama_status, ollama_color) = if ai_check.available {
            ("✓", Color::Green)
        } else {
            ("✗", Color::Yellow)
        };

        text.push(Line::from(vec![
            Span::styled(
                format!("[{}] ", ollama_status),
                Style::default().fg(ollama_color),
            ),
            Span::styled("Ollama", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(": "),
            Span::raw(if ai_check.available {
                "installed"
            } else {
                "not installed"
            }),
        ]));

        // Available models (truncate long lists)
        if !ai_check.models.is_empty() {
            let models_str = ai_check.models.join(", ");
            let display = if models_str.len() > 60 {
                let truncated: String = models_str.chars().take(57).collect();
                format!(
                    "    Models: {}... ({} total)",
                    truncated,
                    ai_check.models.len()
                )
            } else {
                format!("    Models: {}", models_str)
            };
            text.push(Line::from(Span::styled(
                display,
                Style::default().fg(Color::Green),
            )));
        }

        // Recommendations
        if ai_check.available && !ai_check.has_code_model() {
            text.push(Line::from(Span::styled(
                "    ⚠ No code-review model found",
                Style::default().fg(Color::Yellow),
            )));
            text.push(Line::from(Span::styled(
                "    Run: ollama pull llama3.2",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // AI Setup instructions
        if let Some(instructions) = &ai_check.setup_instructions {
            text.push(Line::from(""));
            for line in instructions.lines() {
                text.push(Line::from(Span::styled(
                    format!("    {}", line),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // Summary
        text.push(Line::from(""));
        text.push(Line::from(""));
        let runtime_count = runtime_checks.iter().filter(|c| c.available).count();
        let total_runtime = runtime_checks.len();

        text.push(Line::from(Span::styled(
            format!(
                "Verification Languages: {}/{}",
                runtime_count, total_runtime
            ),
            Style::default()
                .fg(if runtime_count >= 2 {
                    Color::Green
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        )));

        let ai_status = if ai_check.available && ai_check.has_code_model() {
            ("Ready", Color::Green)
        } else if ai_check.available {
            ("Needs model", Color::Yellow)
        } else {
            ("Optional", Color::DarkGray)
        };

        text.push(Line::from(vec![
            Span::raw("AI Code Review: "),
            Span::styled(
                ai_status.0,
                Style::default()
                    .fg(ai_status.1)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            "Note: AI review is optional. You can also use GPT-4, Claude, or other APIs.",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));

        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            "Press any key to return to menu",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Environment Check "),
            );

        frame.render_widget(paragraph, centered_rect(85, 90, area));
    }
}

/// Helper to format Python version status
fn python_status_str(version: Option<&str>) -> String {
    version.map_or("standalone".to_string(), |v| v.to_string())
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn run() -> io::Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new();

    // Main loop
    loop {
        // Check if we need to run setup (which requires dropping terminal)
        if app.should_run_setup() {
            // Temporarily restore terminal for native output
            terminal::disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;

            // Run setup with normal terminal output
            app.run_environment_setup_inner();

            // Restore TUI
            execute!(terminal.backend_mut(), EnterAlternateScreen, cursor::Hide)?;
            terminal::enable_raw_mode()?;
            terminal.clear()?;
        }

        terminal.draw(|f| app.ui(f))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_input(key.code, key.modifiers) {
                    break;
                }
            }
        }
    }

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;

    // Print farewell
    println!("\n  Thanks for using HardClaw!");
    println!("  \"We do not trust; we verify.\"\n");

    Ok(())
}
