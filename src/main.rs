use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, eyre};
use tokio::sync::mpsc;

use mywaker::app::{App, opencode_background};
use mywaker::config::Config;
use mywaker::event::EventHandler;
use mywaker::logging::init_logging;
use mywaker::message::Message;
use mywaker::tui::Tui;

#[derive(Parser)]
#[command(name = "mywaker")]
#[command(about = "Run a team of AI employees backed by OpenCode")]
struct Cli {
    /// Config file path
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List available models and assign one to an employee (the model selector).
    Model {
        #[command(subcommand)]
        cmd: ModelCmd,
    },
    /// Discover projects under ~/Projects (and any configured scan dirs).
    Projects {
        #[command(subcommand)]
        cmd: Option<ProjectsCmd>,
    },
    /// Manage the employee roster.
    Employee {
        #[command(subcommand)]
        cmd: EmployeeCmd,
    },
    /// Discover / install skills from the agent-skills ecosystem (`npx skills`).
    Skills {
        #[command(subcommand)]
        cmd: SkillsCmd,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    /// List models available through OpenCode's installed providers.
    List,
    /// Assign a model to an employee (persisted to team.toml).
    Set { employee: String, model: String },
}

#[derive(Subcommand)]
enum EmployeeCmd {
    /// List employees (with their skills).
    List,
    /// Add an employee.
    Add {
        id: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        role: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Remove an employee.
    Rm { id: String },
    /// Update an employee's name/role/model.
    Set {
        id: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        role: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Attach an installed skill to an employee.
    Skill { id: String, skill: String },
    /// Detach a skill from an employee.
    Unskill { id: String, skill: String },
}

#[derive(Subcommand)]
enum SkillsCmd {
    /// Search the ecosystem for skills (interactive).
    Find { query: Vec<String> },
    /// List installed skills.
    List,
    /// Install a skill package.
    Add {
        package: String,
        #[arg(short, long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum ProjectsCmd {
    /// List projects discovered across all configured scan dirs.
    List,
    /// Manage the directories scanned for projects.
    Dirs {
        #[command(subcommand)]
        cmd: DirsCmd,
    },
}

#[derive(Subcommand)]
enum DirsCmd {
    /// List the directories currently scanned for projects.
    List,
    /// Add one or more directories to scan.
    Add { dirs: Vec<std::path::PathBuf> },
    /// Remove one or more directories from the scan list.
    Rm { dirs: Vec<std::path::PathBuf> },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Model { cmd }) => return run_model(cmd).await,
        Some(Command::Projects { cmd }) => return run_projects(cmd),
        Some(Command::Employee { cmd }) => return run_employee(cmd),
        Some(Command::Skills { cmd }) => return run_skills(cmd),
        None => {}
    }

    let config = Config::load(cli.config.as_deref())?;
    init_logging(&config, cli.debug)?;

    tracing::info!("starting mywaker");

    let (tx, rx) = mpsc::channel::<Message>(256);

    // Connect to OpenCode and forward live events in the background. A Jev
    // client (when a Kenari API key is configured) auto-answers any form the
    // session asks, so delegations never stall waiting on human input.
    let jev = mywaker::jev::JevClient::from_config(&config.jev);
    tokio::spawn(opencode_background(tx.clone(), jev.clone()));

    let mut tui = Tui::new()?;
    let events = EventHandler::new(config.tick_rate, rx);
    let mut app = App::with_team(config, tx, mywaker::team::Team::load_or_starter());
    app.set_jev(jev);

    tui.enter()?;
    let result = app.run(&mut tui, events).await;
    tui.exit()?;

    result
}

/// `mywaker projects [list|dirs …]`: list projects, or manage the scan dirs.
fn run_projects(cmd: Option<ProjectsCmd>) -> Result<()> {
    match cmd {
        Some(ProjectsCmd::List) | None => list_projects(),
        Some(ProjectsCmd::Dirs { cmd }) => match cmd {
            DirsCmd::List => list_scan_dirs(),
            DirsCmd::Add { dirs } => add_scan_dirs(dirs),
            DirsCmd::Rm { dirs } => rm_scan_dirs(dirs),
        },
    }
}

/// List projects discovered across all configured scan dirs.
fn list_projects() -> Result<()> {
    let discovered = mywaker::projects::discover_all()?;
    if discovered.is_empty() {
        let roots = mywaker::projects::scan_roots();
        let joined = roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("no projects under {joined}");
        return Ok(());
    }

    for project in &discovered {
        println!("{}  {}", project.name, project.path.display());
    }
    Ok(())
}

/// List the directories currently scanned for projects.
fn list_scan_dirs() -> Result<()> {
    for root in mywaker::projects::scan_roots() {
        println!("{}", root.display());
    }
    Ok(())
}

fn add_scan_dirs(dirs: Vec<std::path::PathBuf>) -> Result<()> {
    use mywaker::projects::{ScanDirs, default_roots};

    let path = ScanDirs::default_path().ok_or_else(|| eyre!("no config directory found"))?;
    let mut scan = ScanDirs::load(&path).unwrap_or_default();

    // On first use, keep the default (~/Projects) so `add` extends rather than
    // silently replaces the implicit default.
    if scan.dirs.is_empty() {
        for root in default_roots() {
            scan.add(root);
        }
    }

    for dir in dirs {
        let added = scan.add(dir.clone());
        println!(
            "{} {}",
            if added { "added" } else { "already present" },
            dir.display()
        );
    }
    scan.save(&path)?;
    Ok(())
}

fn rm_scan_dirs(dirs: Vec<std::path::PathBuf>) -> Result<()> {
    use mywaker::projects::ScanDirs;

    let path = ScanDirs::default_path().ok_or_else(|| eyre!("no config directory found"))?;
    let mut scan = ScanDirs::load(&path).unwrap_or_default();
    for dir in dirs {
        let removed = scan.remove(&dir);
        println!(
            "{} {}",
            if removed { "removed" } else { "not present" },
            dir.display()
        );
    }
    scan.save(&path)?;
    Ok(())
}

/// `mywaker model [list|set <employee> <model>]`: the model selector.
async fn run_model(cmd: ModelCmd) -> Result<()> {
    match cmd {
        ModelCmd::List => run_model_list().await,
        ModelCmd::Set { employee, model } => run_model_set(&employee, &model),
    }
}

/// List models available through OpenCode's installed providers, marking any
/// model an employee is currently assigned to.
async fn run_model_list() -> Result<()> {
    use mywaker::opencode::OpenCode;
    use mywaker::team::Team;

    let client = OpenCode::connect()
        .await
        .map_err(|err| eyre!("not connected to OpenCode: {err}"))?;
    let models = client.models().await?;

    let team = Team::load_or_starter();
    let in_use: std::collections::HashSet<String> = team
        .employees()
        .iter()
        .filter_map(|e| e.model.clone())
        .collect();

    if models.is_empty() {
        println!("no models reported by OpenCode (`GET /api/model`)");
        return Ok(());
    }

    for model in &models {
        let mark = if in_use.contains(&model.id) {
            "✓ "
        } else {
            "  "
        };
        let name = model.name.clone().unwrap_or_default();
        let provider = model.provider_id.clone().unwrap_or_else(|| model.vendor());
        let cost = model
            .input_cost()
            .map(|c| format!("${c:.2}/1M in"))
            .unwrap_or_else(|| "free".to_string());
        let mut tags = Vec::new();
        if model.is_tool_capable() {
            tags.push("tools");
        }
        if model.is_reasoning() {
            tags.push("reasoning");
        }
        let context = model
            .context_length()
            .map(|c| format!("{} ctx", c))
            .unwrap_or_default();
        println!(
            "{mark}{}  [{provider}]  {name}  {cost}  {context}{}",
            model.id,
            if tags.is_empty() {
                String::new()
            } else {
                format!("  ({})", tags.join("/"))
            }
        );
    }

    println!();
    println!("a \"✓\" marks an employee's assigned model; assign with:");
    println!("  mywaker model set <employee> <model>");
    Ok(())
}

/// `mywaker model set <employee> <model>`: assign a model (persisted to
/// team.toml). The next session for that employee runs on it.
fn run_model_set(employee: &str, model: &str) -> Result<()> {
    use mywaker::team::Team;

    let mut team = Team::load_or_starter();
    team.employee_mut(employee)
        .ok_or_else(|| eyre!("no employee '{employee}'"))?
        .model = Some(model.to_string());
    team.save(&team_path()?)?;
    println!("assigned model '{model}' to '{employee}'");
    Ok(())
}

/// Manage the employee roster: list / add / rm / set / skill / unskill.
fn run_employee(cmd: EmployeeCmd) -> Result<()> {
    use mywaker::team::{Employee, Team};

    match cmd {
        EmployeeCmd::List => {
            let team = Team::load_or_starter();
            print_team(&team);
        }
        EmployeeCmd::Add {
            id,
            name,
            role,
            model,
        } => {
            let mut team = Team::load_or_starter();
            let mut employee = Employee::new(
                id.clone(),
                name.unwrap_or_else(|| id.clone()),
                role.unwrap_or_else(|| "general".to_string()),
            );
            employee.model = model;
            team.add_employee(employee).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("added employee '{id}'");
        }
        EmployeeCmd::Rm { id } => {
            let mut team = Team::load_or_starter();
            if team.remove_employee(&id) {
                team.save(&team_path()?)?;
                println!("removed employee '{id}'");
            } else {
                return Err(eyre!("no employee '{id}'"));
            }
        }
        EmployeeCmd::Set {
            id,
            name,
            role,
            model,
        } => {
            let mut team = Team::load_or_starter();
            let employee = team
                .employee_mut(&id)
                .ok_or_else(|| eyre!("no employee '{id}'"))?;
            if let Some(name) = name {
                employee.name = name;
            }
            if let Some(role) = role {
                employee.role = role;
            }
            if model.is_some() {
                employee.model = model;
            }
            team.save(&team_path()?)?;
            println!("updated employee '{id}'");
        }
        EmployeeCmd::Skill { id, skill } => {
            let mut team = Team::load_or_starter();
            team.add_skill(&id, &skill).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("attached skill '{skill}' to '{id}'");
        }
        EmployeeCmd::Unskill { id, skill } => {
            let mut team = Team::load_or_starter();
            team.remove_skill(&id, &skill).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("detached skill '{skill}' from '{id}'");
        }
    }

    Ok(())
}

/// Discover / install skills via `npx skills`.
fn run_skills(cmd: SkillsCmd) -> Result<()> {
    use mywaker::skills;

    match cmd {
        SkillsCmd::Find { query } => skills::find(&query),
        SkillsCmd::List => skills::list(),
        SkillsCmd::Add { package, global } => skills::add(&package, global),
    }
}

/// Resolve the team file path (error if the config dir is unavailable).
fn team_path() -> Result<std::path::PathBuf> {
    mywaker::team::Team::default_path().ok_or_else(|| eyre!("could not determine config directory"))
}

fn print_team(team: &mywaker::team::Team) {
    if team.is_empty() {
        println!("(no employees — add one with `mywaker employee add <id>`)");
        return;
    }
    for employee in team.employees() {
        let skills = if employee.skills.is_empty() {
            "none".to_string()
        } else {
            employee.skills.join(", ")
        };
        println!(
            "{:<14} {} — {} — model: {} — skills: {}",
            employee.id,
            employee.name,
            employee.role,
            employee.model.as_deref().unwrap_or("default"),
            skills,
        );
    }
}
