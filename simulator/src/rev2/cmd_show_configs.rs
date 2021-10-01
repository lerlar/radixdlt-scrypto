use clap::{crate_version, App, ArgMatches, SubCommand};

use crate::rev2::*;

/// Constructs a `show-configs` subcommand.
pub fn make_show_configs<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(CMD_SHOW_CONFIGS)
        .about("Show configurations")
        .version(crate_version!())
}

/// Handles a `show-configs` request.
pub fn handle_show_configs(_matches: &ArgMatches) -> Result<(), Error> {
    let configs = get_configs()?;

    println!("Default Account: {:?}", configs.default_account);
    println!("Current Epoch: {}", configs.current_epoch);
    println!("Nonce: {}", configs.nonce);

    Ok(())
}
