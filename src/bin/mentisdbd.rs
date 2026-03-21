//! Standalone MentisDb daemon.
//!
//! This binary starts both:
//!
//! - an MCP server (HTTP and optionally HTTPS)
//! - a REST server (HTTP and optionally HTTPS)
//!
//! Configuration is read from environment variables:
//!
//! - `MENTISDB_DIR`
//! - `MENTISDB_DEFAULT_KEY`
//! - `MENTISDB_DEFAULT_STORAGE_ADAPTER`
//! - `MENTISDB_VERBOSE` (defaults to `true` when unset)
//! - `MENTISDB_LOG_FILE`
//! - `MENTISDB_BIND_HOST`
//! - `MENTISDB_MCP_PORT`
//! - `MENTISDB_REST_PORT`
//! - `MENTISDB_HTTPS_MCP_PORT` (set to 0 to disable; default 9473)
//! - `MENTISDB_HTTPS_REST_PORT` (set to 0 to disable; default 9474)
//! - `MENTISDB_TLS_CERT` (default `~/.mentisdb/tls/cert.pem`)
//! - `MENTISDB_TLS_KEY` (default `~/.mentisdb/tls/key.pem`)
//! - `RUST_LOG`

use env_logger::Env;
use mentisdb::server::{
    adopt_legacy_default_mentisdb_dir, start_servers, MentisDbServerConfig, MentisDbServerHandles,
};
use mentisdb::{
    load_registered_chains, migrate_registered_chains_with_adapter, migrate_skill_registry,
    MentisDb, MentisDbMigrationEvent,
};

const MENTIS_BANNER: &str = r#"███╗   ███╗███████╗███╗   ██╗████████╗██╗███████╗
████╗ ████║██╔════╝████╗  ██║╚══██╔══╝██║██╔════╝
██╔████╔██║█████╗  ██╔██╗ ██║   ██║   ██║███████╗
██║╚██╔╝██║██╔══╝  ██║╚██╗██║   ██║   ██║╚════██║
██║ ╚═╝ ██║███████╗██║ ╚████║   ██║   ██║███████║
╚═╝     ╚═╝╚══════╝╚═╝  ╚═══╝   ╚═╝   ╚═╝╚══════╝"#;
const DB_BANNER: &str = r#"██████╗ ██████╗ 
██╔══██╗██╔══██╗
██║  ██║██████╔╝
██║  ██║██╔══██╗
██████╔╝██████╔╝
╚═════╝ ╚═════╝ "#;
const GREEN: &str = "\x1b[38;5;82m";
const YELLOW: &str = "\x1b[38;5;226m";
const PINK: &str = "\x1b[38;5;213m";
const RESET: &str = "\x1b[0m";

pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_logger();
    let storage_root_migration = if std::env::var_os("MENTISDB_DIR").is_none() {
        adopt_legacy_default_mentisdb_dir()?
    } else {
        None
    };
    let config = MentisDbServerConfig::from_env();

    print_banner();
    println!("mentisdb v{}", env!("CARGO_PKG_VERSION"));
    println!("mentisdbd starting");
    if let Some(report) = &storage_root_migration {
        println!("Legacy storage adoption:");
        if report.renamed_root_dir {
            println!(
                "  Renamed {} -> {}",
                report.source_dir.display(),
                report.target_dir.display()
            );
        } else {
            println!(
                "  Merged {} legacy entries from {} into {}",
                report.merged_entries,
                report.source_dir.display(),
                report.target_dir.display()
            );
        }
        if report.renamed_registry_file {
            println!("  Renamed thoughtchain-registry.json -> mentisdb-registry.json");
        }
    }
    println!("Configuration:");
    print_env_var(
        "MENTISDB_DIR",
        Some(config.service.chain_dir.display().to_string()),
    );
    print_env_var(
        "MENTISDB_DEFAULT_KEY",
        Some(config.service.default_chain_key.clone()),
    );
    print_env_var(
        "MENTISDB_DEFAULT_STORAGE_ADAPTER",
        Some(config.service.default_storage_adapter.to_string()),
    );
    print_env_var("MENTISDB_VERBOSE", Some(config.service.verbose.to_string()));
    print_env_var(
        "MENTISDB_LOG_FILE",
        config
            .service
            .log_file
            .as_ref()
            .map(|path| path.display().to_string()),
    );
    print_env_var("MENTISDB_BIND_HOST", Some(config.mcp_addr.ip().to_string()));
    print_env_var(
        "MENTISDB_MCP_PORT",
        Some(config.mcp_addr.port().to_string()),
    );
    print_env_var(
        "MENTISDB_REST_PORT",
        Some(config.rest_addr.port().to_string()),
    );
    print_env_var(
        "MENTISDB_HTTPS_MCP_PORT",
        Some(match config.https_mcp_addr {
            Some(addr) => addr.port().to_string(),
            None => "disabled".to_string(),
        }),
    );
    print_env_var(
        "MENTISDB_HTTPS_REST_PORT",
        Some(match config.https_rest_addr {
            Some(addr) => addr.port().to_string(),
            None => "disabled".to_string(),
        }),
    );
    print_env_var(
        "MENTISDB_TLS_CERT",
        Some(config.tls_cert_path.display().to_string()),
    );
    print_env_var(
        "MENTISDB_TLS_KEY",
        Some(config.tls_key_path.display().to_string()),
    );

    let migration_reports = migrate_registered_chains_with_adapter(
        &config.service.chain_dir,
        config.service.default_storage_adapter,
        |event| match event {
            MentisDbMigrationEvent::Started {
                chain_key,
                from_version,
                to_version,
                current,
                total,
            } => println!(
                "{} Migrating chain {} from version {} to version {}",
                progress_bar(current, total),
                chain_key,
                from_version,
                to_version
            ),
            MentisDbMigrationEvent::Completed {
                chain_key,
                from_version,
                to_version,
                current,
                total,
            } => println!(
                "{} Migrated chain {} from version {} to version {}",
                progress_bar(current, total),
                chain_key,
                from_version,
                to_version
            ),
            MentisDbMigrationEvent::StartedReconciliation {
                chain_key,
                from_storage_adapter,
                to_storage_adapter,
                current,
                total,
            } => println!(
                "{} Reconciling chain {} from {} storage to {} storage",
                progress_bar(current, total),
                chain_key,
                from_storage_adapter,
                to_storage_adapter
            ),
            MentisDbMigrationEvent::CompletedReconciliation {
                chain_key,
                from_storage_adapter,
                to_storage_adapter,
                current,
                total,
            } => println!(
                "{} Reconciled chain {} from {} storage to {} storage",
                progress_bar(current, total),
                chain_key,
                from_storage_adapter,
                to_storage_adapter
            ),
        },
    )?;
    if migration_reports.is_empty() {
        println!("No chain migrations required.");
    }

    // Skill registry migration — must run before start_servers opens the registry.
    match migrate_skill_registry(&config.service.chain_dir) {
        Ok(None) => println!("Skill registry: up to date, no migration required."),
        Ok(Some(report)) => println!(
            "Skill registry migrated: {} skill(s), {} version(s) converted (v{} → v{}) at {}",
            report.skills_migrated,
            report.versions_migrated,
            report.from_version,
            report.to_version,
            report.path.display()
        ),
        Err(e) => panic!("Skill registry migration failed — cannot start server: {e}"),
    }

    let handles = start_servers(config.clone()).await?;

    println!("mentisdbd running");
    println!("Resolved endpoints:");
    println!("MCP server:  http://{}", handles.mcp.local_addr());
    println!("REST server: http://{}", handles.rest.local_addr());
    if let Some(ref h) = handles.https_mcp {
        println!("MCP server:  https://{}", h.local_addr());
    }
    if let Some(ref h) = handles.https_rest {
        println!("REST server: https://{}", h.local_addr());
    }
    print_endpoint_catalog(&handles);
    print_chain_summary(&config)?;
    print_tls_tip(&config, &handles);
    println!("Press Ctrl+C to stop.");

    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[allow(dead_code)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run().await
}

fn print_env_var(name: &str, effective_value: Option<String>) {
    if let Ok(raw_value) = std::env::var(name) {
        println!(
            "  {YELLOW}{name}{RESET}={raw_value} (effective: {GREEN}{}{RESET})",
            display_value(effective_value)
        );
        return;
    }

    println!(
        "  {YELLOW}{name}{RESET}=<unset> (effective default: {GREEN}{}{RESET})",
        display_value(effective_value)
    );
}

fn display_value(value: Option<String>) -> String {
    value.unwrap_or_else(|| "<none>".to_string())
}

fn init_logger() {
    let mut builder = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
    builder.format_timestamp_millis();
    let _ = builder.try_init();
}

fn print_banner() {
    for (mentis, db) in MENTIS_BANNER.lines().zip(DB_BANNER.lines()) {
        println!("{GREEN}{mentis}{RESET} {PINK}{db}{RESET}");
    }
}

fn progress_bar(current: usize, total: usize) -> String {
    let total = total.max(1);
    let current = current.min(total);
    let filled = ((current * 20) / total).min(20);
    format!(
        "[{}{}] {}/{}",
        "#".repeat(filled),
        "-".repeat(20 - filled),
        current,
        total
    )
}

fn print_endpoint_catalog(handles: &MentisDbServerHandles) {
    println!();
    println!("Endpoints:");
    println!("  MCP");
    println!("    POST http://{}", handles.mcp.local_addr());
    println!("      Standard streamable HTTP MCP root endpoint.");
    println!("    GET  http://{}/health", handles.mcp.local_addr());
    println!("      Health check for the MCP surface.");
    println!("    POST http://{}/tools/list", handles.mcp.local_addr());
    println!("      Legacy CloudLLM-compatible MCP tool discovery.");
    println!("    POST http://{}/tools/execute", handles.mcp.local_addr());
    println!("      Legacy CloudLLM-compatible MCP tool execution.");
    println!("  REST");
    println!("    GET  http://{}/health", handles.rest.local_addr());
    println!("      Health check for the REST surface.");
    println!("    GET  http://{}/v1/chains", handles.rest.local_addr());
    println!("      List chains with version, adapter, counts, and storage location.");
    println!("    POST http://{}/v1/agents", handles.rest.local_addr());
    println!("      List agent identity summaries for one chain.");
    println!("    POST http://{}/v1/agent", handles.rest.local_addr());
    println!("      Return one full agent registry record.");
    println!(
        "    POST http://{}/v1/agent-registry",
        handles.rest.local_addr()
    );
    println!("      Return the full agent registry for one chain.");
    println!(
        "    POST http://{}/v1/agents/upsert",
        handles.rest.local_addr()
    );
    println!("      Create or update an agent registry record.");
    println!(
        "    POST http://{}/v1/agents/description",
        handles.rest.local_addr()
    );
    println!("      Set or clear one agent description.");
    println!(
        "    POST http://{}/v1/agents/aliases",
        handles.rest.local_addr()
    );
    println!("      Add one alias to a registered agent.");
    println!(
        "    POST http://{}/v1/agents/keys",
        handles.rest.local_addr()
    );
    println!("      Add or replace one agent public key.");
    println!(
        "    POST http://{}/v1/agents/keys/revoke",
        handles.rest.local_addr()
    );
    println!("      Revoke one agent public key.");
    println!(
        "    POST http://{}/v1/agents/disable",
        handles.rest.local_addr()
    );
    println!("      Disable one registered agent.");
    println!(
        "    GET  http://{}/mentisdb_skill_md",
        handles.rest.local_addr()
    );
    println!("      Return the embedded official MentisDB skill Markdown.");
    println!("    GET  http://{}/v1/skills", handles.rest.local_addr());
    println!("      List uploaded skill summaries from the registry.");
    println!(
        "    GET  http://{}/v1/skills/manifest",
        handles.rest.local_addr()
    );
    println!("      Describe searchable fields and supported skill formats.");
    println!(
        "    POST http://{}/v1/skills/upload",
        handles.rest.local_addr()
    );
    println!("      Upload a new immutable skill version.");
    println!(
        "    POST http://{}/v1/skills/search",
        handles.rest.local_addr()
    );
    println!("      Search skills by metadata, uploader identity, and time window.");
    println!(
        "    POST http://{}/v1/skills/read",
        handles.rest.local_addr()
    );
    println!("      Read one stored skill as Markdown or JSON with safety warnings.");
    println!(
        "    POST http://{}/v1/skills/versions",
        handles.rest.local_addr()
    );
    println!("      List immutable uploaded versions for one skill.");
    println!(
        "    POST http://{}/v1/skills/deprecate",
        handles.rest.local_addr()
    );
    println!("      Mark one skill as deprecated.");
    println!(
        "    POST http://{}/v1/skills/revoke",
        handles.rest.local_addr()
    );
    println!("      Mark one skill as revoked.");
    println!("    POST http://{}/v1/bootstrap", handles.rest.local_addr());
    println!("      Bootstrap an empty chain with an initial checkpoint.");
    println!("    POST http://{}/v1/thoughts", handles.rest.local_addr());
    println!("      Append a durable thought.");
    println!(
        "    POST http://{}/v1/retrospectives",
        handles.rest.local_addr()
    );
    println!("      Append a retrospective thought.");
    println!("    POST http://{}/v1/search", handles.rest.local_addr());
    println!("      Search thoughts by semantic and identity filters.");
    println!(
        "    POST http://{}/v1/recent-context",
        handles.rest.local_addr()
    );
    println!("      Render a recent-context prompt snippet.");
    println!(
        "    POST http://{}/v1/memory-markdown",
        handles.rest.local_addr()
    );
    println!("      Export a MEMORY.md-style markdown view.");
    println!("    POST http://{}/v1/thought", handles.rest.local_addr());
    println!("      Read one thought by id, hash, or append-order index.");
    println!(
        "    POST http://{}/v1/thoughts/genesis",
        handles.rest.local_addr()
    );
    println!("      Return the first thought in append order.");
    println!(
        "    POST http://{}/v1/thoughts/traverse",
        handles.rest.local_addr()
    );
    println!("      Traverse thoughts forward or backward in filtered chunks.");
    println!("    POST http://{}/v1/head", handles.rest.local_addr());
    println!("      Return the latest thought at the chain tip and head metadata.");
    println!();

    if let Some(https_mcp) = &handles.https_mcp {
        println!("  HTTPS MCP");
        println!("    POST https://{}", https_mcp.local_addr());
        println!("      Streamable HTTP MCP root endpoint over TLS.");
        println!("    GET  https://{}/health", https_mcp.local_addr());
        println!("      Health check for the HTTPS MCP surface.");
        println!("    POST https://{}/tools/list", https_mcp.local_addr());
        println!("      Legacy CloudLLM-compatible MCP tool discovery (HTTPS).");
        println!("    POST https://{}/tools/execute", https_mcp.local_addr());
        println!("      Legacy CloudLLM-compatible MCP tool execution (HTTPS).");
    }
    if let Some(https_rest) = &handles.https_rest {
        println!("  HTTPS REST");
        println!("    GET  https://{}/health", https_rest.local_addr());
        println!("      Health check for the HTTPS REST surface.");
        println!("    GET  https://{}/v1/chains", https_rest.local_addr());
        println!("      List chains with version, adapter, counts, and storage location.");
        println!("    POST https://{}/v1/agents", https_rest.local_addr());
        println!("      List agent identity summaries for one chain.");
        println!("    POST https://{}/v1/agent", https_rest.local_addr());
        println!("      Return one full agent registry record.");
        println!(
            "    POST https://{}/v1/agent-registry",
            https_rest.local_addr()
        );
        println!("      Return the full agent registry for one chain.");
        println!(
            "    POST https://{}/v1/agents/upsert",
            https_rest.local_addr()
        );
        println!("      Create or update an agent registry record.");
        println!(
            "    POST https://{}/v1/agents/description",
            https_rest.local_addr()
        );
        println!("      Set or clear one agent description.");
        println!(
            "    POST https://{}/v1/agents/aliases",
            https_rest.local_addr()
        );
        println!("      Add one alias to a registered agent.");
        println!(
            "    POST https://{}/v1/agents/keys",
            https_rest.local_addr()
        );
        println!("      Add or replace one agent public key.");
        println!(
            "    POST https://{}/v1/agents/keys/revoke",
            https_rest.local_addr()
        );
        println!("      Revoke one agent public key.");
        println!(
            "    POST https://{}/v1/agents/disable",
            https_rest.local_addr()
        );
        println!("      Disable one registered agent.");
        println!(
            "    GET  https://{}/mentisdb_skill_md",
            https_rest.local_addr()
        );
        println!("      Return the embedded official MentisDB skill Markdown.");
        println!("    GET  https://{}/v1/skills", https_rest.local_addr());
        println!("      List uploaded skill summaries from the registry.");
        println!(
            "    GET  https://{}/v1/skills/manifest",
            https_rest.local_addr()
        );
        println!("      Describe searchable fields and supported skill formats.");
        println!(
            "    POST https://{}/v1/skills/upload",
            https_rest.local_addr()
        );
        println!("      Upload a new immutable skill version.");
        println!(
            "    POST https://{}/v1/skills/search",
            https_rest.local_addr()
        );
        println!("      Search skills by metadata, uploader identity, and time window.");
        println!(
            "    POST https://{}/v1/skills/read",
            https_rest.local_addr()
        );
        println!("      Read one stored skill as Markdown or JSON with safety warnings.");
        println!(
            "    POST https://{}/v1/skills/versions",
            https_rest.local_addr()
        );
        println!("      List immutable uploaded versions for one skill.");
        println!(
            "    POST https://{}/v1/skills/deprecate",
            https_rest.local_addr()
        );
        println!("      Mark one skill as deprecated.");
        println!(
            "    POST https://{}/v1/skills/revoke",
            https_rest.local_addr()
        );
        println!("      Mark one skill as revoked.");
        println!(
            "    POST https://{}/v1/bootstrap",
            https_rest.local_addr()
        );
        println!("      Bootstrap an empty chain with an initial checkpoint.");
        println!("    POST https://{}/v1/thoughts", https_rest.local_addr());
        println!("      Append a durable thought.");
        println!(
            "    POST https://{}/v1/retrospectives",
            https_rest.local_addr()
        );
        println!("      Append a retrospective thought.");
        println!("    POST https://{}/v1/search", https_rest.local_addr());
        println!("      Search thoughts by semantic and identity filters.");
        println!(
            "    POST https://{}/v1/recent-context",
            https_rest.local_addr()
        );
        println!("      Render a recent-context prompt snippet.");
        println!(
            "    POST https://{}/v1/memory-markdown",
            https_rest.local_addr()
        );
        println!("      Export a MEMORY.md-style markdown view.");
        println!("    POST https://{}/v1/thought", https_rest.local_addr());
        println!("      Read one thought by id, hash, or append-order index.");
        println!(
            "    POST https://{}/v1/thoughts/genesis",
            https_rest.local_addr()
        );
        println!("      Return the first thought in append order.");
        println!(
            "    POST https://{}/v1/thoughts/traverse",
            https_rest.local_addr()
        );
        println!("      Traverse thoughts forward or backward in filtered chunks.");
        println!("    POST https://{}/v1/head", https_rest.local_addr());
        println!("      Return the latest thought at the chain tip and head metadata.");
        println!();
    }
}

fn print_chain_summary(
    config: &MentisDbServerConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let registry = load_registered_chains(&config.service.chain_dir)?;
    println!("Chain Summary:");
    if registry.chains.is_empty() {
        println!("  No registered chains.");
        println!();
        return Ok(());
    }

    for entry in registry.chains.values() {
        println!(
            "  {} | version {} | {} | {} thoughts | {} agents",
            entry.chain_key,
            entry.version,
            entry.storage_adapter,
            entry.thought_count,
            entry.agent_count
        );
        println!("    {}", entry.storage_location);

        match MentisDb::open_with_storage(
            entry
                .storage_adapter
                .for_chain_key(&config.service.chain_dir, &entry.chain_key),
        ) {
            Ok(chain) => {
                for agent in chain.list_agent_registry() {
                    let description = agent
                        .description
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("no description");
                    println!(
                        "    - {} [{}] | {} | {} thought(s) | {}",
                        agent.display_name,
                        agent.agent_id,
                        agent.status,
                        agent.thought_count,
                        description
                    );
                }
            }
            Err(error) => println!("    Unable to open chain summary: {error}"),
        }
        println!();
    }

    Ok(())
}

/// Prints TLS certificate trust instructions and the `my.mentisdb.com` tip,
/// but only when at least one HTTPS listener is active.
///
/// `my.mentisdb.com` is a public DNS A-record that resolves to `127.0.0.1`,
/// providing a human-friendly hostname for the local daemon once the
/// self-signed certificate has been trusted.
fn print_tls_tip(config: &MentisDbServerConfig, handles: &MentisDbServerHandles) {
    if handles.https_mcp.is_none() && handles.https_rest.is_none() {
        return;
    }

    let mcp_port = handles.https_mcp.as_ref().map(|h| h.local_addr().port());
    let rest_port = handles.https_rest.as_ref().map(|h| h.local_addr().port());

    println!("TLS Certificate: {}", config.tls_cert_path.display());
    println!();
    println!(
        "  {YELLOW}my.mentisdb.com{RESET} is a public DNS A-record \u{2192} 127.0.0.1"
    );
    println!("  You can use it as a friendly hostname for this local daemon.");
    if let Some(port) = mcp_port {
        println!("  MCP:  https://my.mentisdb.com:{port}");
    }
    if let Some(port) = rest_port {
        println!("  REST: https://my.mentisdb.com:{port}");
    }
    println!();
    println!("  To avoid certificate warnings, trust the self-signed cert once:");
    println!(
        "  {GREEN}macOS{RESET}:   sudo security add-trusted-cert -d -r trustRoot \\"
    );
    println!(
        "             -k /Library/Keychains/System.keychain \\"
    );
    println!(
        "             {}",
        config.tls_cert_path.display()
    );
    println!(
        "  {GREEN}Linux{RESET}:   sudo cp {} /usr/local/share/ca-certificates/mentisdb.crt",
        config.tls_cert_path.display()
    );
    println!("           sudo update-ca-certificates");
    println!(
        "  {GREEN}Windows{RESET}: certutil -addstore Root {}",
        config.tls_cert_path.display()
    );
    println!();
}