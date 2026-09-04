//! コマンド定義。引数と既定値は TypeScript 版の commander 定義をそのまま移した。

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
// 版はビルド時に決まる（`build.rs`。タグからのリリースではタグの版になる）
#[command(name = "task", version = env!("TASK_CLI_VERSION"), about = "Task management CLI")]
pub struct Cli {
    /// Output JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Authentication commands
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Manage ~/.config/task/config.yaml
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Project commands
    Projects {
        #[command(subcommand)]
        command: ProjectsCommand,
    },
    /// Task commands
    Tasks {
        // 一番大きい派生（`Update` の項目群）に enum 全体の大きさが引きずられるので箱に入れる
        #[command(subcommand)]
        command: Box<TasksCommand>,
    },
    /// My Tasks commands
    My {
        #[command(subcommand)]
        command: MyCommand,
    },
    /// Sprint commands
    Sprints {
        #[command(subcommand)]
        command: SprintsCommand,
    },
    /// Review findings commands
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Show current user
    Whoami,
    /// Save personal access token to config
    Token {
        /// Token value (omit to read from stdin)
        token: Option<String>,
    },
    /// Remove token from local config
    Logout,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List all config values
    List,
    /// Get a config value
    Get {
        /// api_url | token | tenant_id
        key: String,
    },
    /// Set a config value
    Set {
        /// api_url | token | tenant_id
        key: String,
        /// Value to store
        value: String,
    },
    /// Remove a config value
    Unset {
        /// api_url | token | tenant_id
        key: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommand {
    /// List projects
    List,
    /// Show a project by key or UUID
    Show {
        /// Project key or UUID
        key: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum TasksCommand {
    /// List tasks in a project
    List {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// Filter by priority
        #[arg(long)]
        priority: Option<String>,
        /// Filter by status name
        #[arg(long)]
        status: Option<String>,
        /// Filter by label name
        #[arg(long)]
        label: Option<String>,
        /// Filter by assignee username (or UUID)
        #[arg(long)]
        assignee: Option<String>,
        /// Filter by milestone name (or UUID)
        #[arg(long)]
        milestone: Option<String>,
        /// Filter by sprint name (or UUID)
        #[arg(long)]
        sprint: Option<String>,
        /// Filter by parent task (KEY-N or UUID)
        #[arg(long)]
        parent: Option<String>,
        /// List archived tasks instead of active ones
        #[arg(long)]
        archived: bool,
        /// created_at_desc | priority_asc | deadline_asc
        #[arg(long)]
        sort: Option<String>,
        /// Tasks per page (1-200)
        #[arg(long, default_value_t = 50)]
        limit: u64,
        /// Page number, 1-based
        #[arg(long, default_value_t = 1)]
        page: u64,
    },
    /// Search tasks by text
    Search {
        /// Search query
        query: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// Hits per page (1-100)
        #[arg(long, default_value_t = 20)]
        limit: u64,
        /// Page number, 1-based
        #[arg(long, default_value_t = 1)]
        page: u64,
    },
    /// Create a task
    Create {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// Task title
        #[arg(long)]
        title: String,
        /// Task description
        #[arg(long)]
        description: Option<String>,
        /// Read the description from a file (`-` for stdin)
        #[arg(long, conflicts_with = "description")]
        description_file: Option<String>,
        /// Task priority
        #[arg(long)]
        priority: Option<String>,
        /// Status name
        #[arg(long)]
        status: Option<String>,
        #[command(flatten)]
        fields: TaskFieldArgs,
    },
    /// Show a task
    Show {
        /// Task ref (KEY-N or UUID)
        task_ref: String,
        /// Project key when using UUID
        #[arg(long)]
        project: Option<String>,
    },
    /// Update a task
    Update {
        /// Task ref (KEY-N or UUID)
        task_ref: String,
        /// Project key when using UUID
        #[arg(long)]
        project: Option<String>,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Read the new description from a file (`-` for stdin)
        #[arg(long, conflicts_with = "description")]
        description_file: Option<String>,
        /// Status name
        #[arg(long)]
        status: Option<String>,
        /// Priority
        #[arg(long)]
        priority: Option<String>,
        #[command(flatten)]
        fields: TaskFieldArgs,
        #[command(flatten)]
        clears: TaskClearArgs,
        /// Move the task into the archive
        #[arg(long)]
        archive: bool,
        /// Bring the task back from the archive
        #[arg(long, conflicts_with = "archive")]
        unarchive: bool,
    },
    /// Mark a task as done
    Complete {
        /// Task ref (KEY-N or UUID)
        task_ref: String,
        /// Project key when using UUID
        #[arg(long)]
        project: Option<String>,
    },
    /// Add a comment to a task
    Comment {
        /// Task ref (KEY-N or UUID)
        task_ref: String,
        /// Comment body (omit to read from stdin)
        body: Option<String>,
        /// Read the comment body from a file (`-` for stdin)
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Project key when using UUID
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a task
    Delete {
        /// Task ref (KEY-N or UUID)
        task_ref: String,
        /// Project key when using UUID
        #[arg(long)]
        project: Option<String>,
    },
}

/// 作成と更新で同じ綴りにする項目。片方だけ増えると使う側が覚え直しになる。
#[derive(Debug, clap::Args)]
pub struct TaskFieldArgs {
    /// Soft deadline (RFC 3339, or YYYY-MM-DD for end of that day in UTC)
    #[arg(long)]
    pub soft_deadline: Option<String>,
    /// Hard deadline (RFC 3339, or YYYY-MM-DD for end of that day in UTC)
    #[arg(long)]
    pub hard_deadline: Option<String>,
    /// Estimate in minutes (1 or more)
    #[arg(long)]
    pub estimate: Option<i32>,
    /// Progress percentage (0-100)
    #[arg(long)]
    pub progress: Option<i16>,
    /// Parent task (KEY-N or UUID)
    #[arg(long)]
    pub parent: Option<String>,
    /// Milestone name (or UUID)
    #[arg(long)]
    pub milestone: Option<String>,
    /// Sprint name (or UUID)
    #[arg(long)]
    pub sprint: Option<String>,
    /// Label name; repeat to set several (replaces the current labels)
    #[arg(long = "label")]
    pub labels: Vec<String>,
    /// Label to add, keeping the others; repeat to add several
    #[arg(long = "add-label", conflicts_with = "labels")]
    pub add_labels: Vec<String>,
    /// Label to remove, keeping the others; repeat to remove several
    #[arg(long = "remove-label", conflicts_with = "labels")]
    pub remove_labels: Vec<String>,
    /// Assignee username; repeat to set several (replaces the current assignees)
    #[arg(long = "assignee")]
    pub assignees: Vec<String>,
}

/// 明示的な解除。値の指定と混ぜられないよう `conflicts_with` を張る。
#[derive(Debug, clap::Args)]
pub struct TaskClearArgs {
    /// Remove the description
    #[arg(long, conflicts_with_all = ["description", "description_file"])]
    pub clear_description: bool,
    /// Remove the soft deadline
    #[arg(long, conflicts_with = "soft_deadline")]
    pub clear_soft_deadline: bool,
    /// Remove the hard deadline
    #[arg(long, conflicts_with = "hard_deadline")]
    pub clear_hard_deadline: bool,
    /// Remove the estimate
    #[arg(long, conflicts_with = "estimate")]
    pub clear_estimate: bool,
    /// Detach from the parent task
    #[arg(long, conflicts_with = "parent")]
    pub clear_parent: bool,
    /// Detach from the milestone
    #[arg(long, conflicts_with = "milestone")]
    pub clear_milestone: bool,
    /// Detach from the sprint
    #[arg(long, conflicts_with = "sprint")]
    pub clear_sprint: bool,
}

#[derive(Debug, Subcommand)]
pub enum MyCommand {
    /// List tasks assigned to me
    List {
        /// today | week | no_due_date | overdue | all
        #[arg(long, default_value = "all")]
        filter: String,
    },
    /// Complete a personal or assigned task by ref (e.g. ME-3)
    Complete {
        /// Task ref (ME-N, KEY-N, or UUID)
        task_ref: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SprintsCommand {
    /// List sprints
    List {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// Filter by sprint status
        #[arg(long)]
        status: Option<String>,
    },
    /// Show sprint details
    Show {
        /// Sprint UUID
        id: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
    },
    /// Start a sprint
    Start {
        /// Sprint UUID
        id: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
    },
    /// Complete a sprint
    Complete {
        /// Sprint UUID
        id: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// Move incomplete tasks to backlog
        #[arg(long)]
        backlog: bool,
    },
    /// Show sprint burndown data
    Burndown {
        /// Sprint UUID or name
        id: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReviewCommand {
    /// Submit one review round (findings are created together)
    Submit {
        /// JSON file with the round, or - for stdin
        file: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// PR number (overrides `pr` in the JSON)
        #[arg(long)]
        pr: Option<String>,
    },
    /// List findings for a PR
    List {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// PR number
        #[arg(long)]
        pr: String,
        /// Repository to read (default: the current integration; "" for rounds recorded before it)
        #[arg(long, value_name = "owner/name")]
        repo: Option<String>,
        /// Filter by state (open,fixed,verified,deferred,rejected)
        #[arg(long, value_name = "states")]
        state: Option<String>,
        /// Filter by severity (high,medium,low,nit)
        #[arg(long, value_name = "severities")]
        severity: Option<String>,
    },
    /// List review rounds for a PR
    Rounds {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// PR number
        #[arg(long)]
        pr: String,
        /// Repository to read (default: the current integration; "" for rounds recorded before it)
        #[arg(long, value_name = "owner/name")]
        repo: Option<String>,
    },
    /// Move a finding to a new state
    Resolve {
        /// Finding UUID
        id: String,
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// New state (open,fixed,verified,deferred,rejected)
        #[arg(long)]
        state: String,
        /// Why the state changed (kept in the history)
        #[arg(long, value_name = "text")]
        note: Option<String>,
    },
    /// Show the merge verdict for a PR (exits 1 unless it is reviewed, clean, and up to date)
    Summary {
        /// Project key or UUID
        #[arg(long)]
        project: String,
        /// PR number
        #[arg(long)]
        pr: String,
        /// Repository to read (default: the current integration; "" for rounds recorded before it)
        #[arg(long, value_name = "owner/name")]
        repo: Option<String>,
        /// Commit to compare with the reviewed HEAD (default: git rev-parse HEAD)
        #[arg(long, value_name = "sha")]
        head: Option<String>,
        /// Do not compare the reviewed HEAD with the working tree
        #[arg(long = "no-head-check")]
        no_head_check: bool,
        /// Accept a summary from a project without a GitHub integration
        #[arg(long)]
        allow_unlinked: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    /// 版はビルド時に決まる。既定はクレートの版で、リリースではタグの版が入る。
    #[test]
    fn reports_the_version_it_was_built_with() {
        let version = Cli::command().get_version().map(str::to_string);
        assert_eq!(version.as_deref(), Some(env!("TASK_CLI_VERSION")));
        assert!(!env!("TASK_CLI_VERSION").is_empty());
    }

    #[test]
    fn accepts_the_json_flag_before_or_after_the_subcommand() {
        for argv in [
            vec!["task", "--json", "projects", "list"],
            vec!["task", "projects", "list", "--json"],
        ] {
            let cli = Cli::try_parse_from(argv).unwrap();
            assert!(cli.json);
        }
    }

    #[test]
    fn head_check_defaults_to_on_and_is_removed_only_by_the_explicit_flag() {
        let on =
            Cli::try_parse_from(["task", "review", "summary", "--project", "APP", "--pr", "1"])
                .unwrap();
        let off = Cli::try_parse_from([
            "task",
            "review",
            "summary",
            "--project",
            "APP",
            "--pr",
            "1",
            "--no-head-check",
        ])
        .unwrap();

        let flag = |cli: Cli| match cli.command {
            Command::Review {
                command: ReviewCommand::Summary { no_head_check, .. },
            } => no_head_check,
            _ => unreachable!(),
        };
        assert!(!flag(on));
        assert!(flag(off));
    }

    #[test]
    fn requires_the_project_and_pr_options_the_read_commands_are_documented_with() {
        assert!(Cli::try_parse_from(["task", "review", "list", "--pr", "1"]).is_err());
        assert!(Cli::try_parse_from(["task", "review", "list", "--project", "APP"]).is_err());
    }

    /// 本文の指定と解除を同時に受けると、API が解除を優先して渡した本文が黙って消える。
    #[test]
    fn refuses_a_new_description_together_with_clearing_it() {
        let base = ["task", "tasks", "update", "APP-1"];
        for value in [
            vec!["--description", "new"],
            vec!["--description-file", "body.md"],
        ] {
            let argv: Vec<&str> = base
                .iter()
                .copied()
                .chain(value)
                .chain(["--clear-description"])
                .collect();
            assert!(Cli::try_parse_from(&argv).is_err(), "{argv:?}");
        }
        // 解除だけ、本文だけならそれぞれ通る
        assert!(
            Cli::try_parse_from(["task", "tasks", "update", "APP-1", "--clear-description"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["task", "tasks", "update", "APP-1", "--description", "new"])
                .is_ok()
        );
    }

    #[test]
    fn keeps_an_empty_repo_filter_as_a_value() {
        let cli = Cli::try_parse_from([
            "task",
            "review",
            "rounds",
            "--project",
            "APP",
            "--pr",
            "1",
            "--repo",
            "",
        ])
        .unwrap();
        match cli.command {
            Command::Review {
                command: ReviewCommand::Rounds { repo, .. },
            } => {
                assert_eq!(repo.as_deref(), Some(""));
            }
            _ => unreachable!(),
        }
    }
}
