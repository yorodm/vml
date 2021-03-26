use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::process::Command;
use std::thread;
use std::time::Duration;

use clap::ArgMatches;

use vml::cli;
use vml::config::Config;
use vml::config::CreateExistsAction;
use vml::files;
use vml::template;
use vml::{Error, Result};
use vml::{VMsCreator, WithPid};

fn list(vmc: &VMsCreator, config: &Config, fold: bool, unfold: bool) -> Result<()> {
    let fold = if config.commands.list.fold { fold || !unfold } else { fold && !unfold };

    let mut names: BTreeSet<String> = BTreeSet::new();

    for vm in vmc.create()? {
        if fold {
            names.insert(vm.folded_name());
        } else {
            names.insert(vm.name.to_owned());
        }
    }

    for name in names {
        println!("{}", name);
    }

    Ok(())
}

fn create(config: &Config, create_matches: &ArgMatches) -> Result<()> {
    let names: Vec<&str> = if create_matches.is_present("names") {
        create_matches.values_of("names").unwrap().collect()
    } else if let Some(name) = create_matches.value_of("NAME") {
        vec![name]
    } else {
        vec![]
    };

    let image = create_matches.value_of("image");

    let exists = if create_matches.is_present("exists-fail") {
        CreateExistsAction::Fail
    } else if create_matches.is_present("exists-ignore") {
        CreateExistsAction::Ignore
    } else if create_matches.is_present("exists-replace") {
        CreateExistsAction::Replace
    } else {
        config.commands.create.exists
    };

    for name in names {
        vml::create_vm(&config, name, image, exists)?;
    }

    Ok(())
}

fn start(config: &Config, start_matches: &ArgMatches, vmc: &mut VMsCreator) -> Result<()> {
    set_specifications(vmc, start_matches);

    let wait_ssh = start_matches.is_present("wait-ssh");
    let cloud_init = config.commands.start.cloud_init
        && !start_matches.is_present("no-cloud-init")
        || start_matches.is_present("cloud-init");
    let drives: Vec<&str> = if let Some(drives) = start_matches.values_of("drives") {
        drives.collect()
    } else {
        vec![]
    };

    vmc.with_pid(WithPid::Without);
    vmc.error_on_empty();

    let vms = vmc.create()?;

    for vm in &vms {
        vm.start(cloud_init, &drives)?;
    }

    if wait_ssh {
        let user: Option<&str> = None;
        let repeat = config.commands.start.wait_ssh.repeat;
        let sleep = config.commands.start.wait_ssh.sleep;
        let options = [
            format!("ConnectionAttempts={}", config.commands.start.wait_ssh.attempts),
            format!("ConnectTimeout={}", config.commands.start.wait_ssh.timeout),
        ];
        let flags: Vec<&str> = vec![];
        for vm in &vms {
            for _ in 0..repeat {
                if vm.ssh(&user, &options, &flags, &Some(vec!["true"]))? == Some(0) {
                    break;
                } else {
                    thread::sleep(Duration::from_secs(sleep));
                }
            }
        }
    }

    Ok(())
}

fn confirm(message: &str) -> bool {
    println!("{}", message);
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Cannot read from stdin");
    let input = input.trim_end();
    let input = input.to_lowercase();

    matches!(input.as_str(), "y" | "yes")
}

fn args_without_host() -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut args_iterator = env::args();
    let mut optional_arg = args_iterator.next();
    let mut found = false;
    while let Some(arg) = &optional_arg {
        if !found && matches!(arg.as_str(), "--host" | "-H") {
            args_iterator.next();
            found = true;
        } else {
            args.push(arg.to_string());
        }
        optional_arg = args_iterator.next();
    }

    args
}

fn parse_user_at_name(user_at_name: &str) -> (Option<&str>, &str) {
    if user_at_name.contains('@') {
        let user_name: Vec<&str> = user_at_name.splitn(2, '@').collect();
        (Some(user_name[0]), user_name[1])
    } else {
        (None, user_at_name)
    }
}

fn set_specifications(vmc: &mut VMsCreator, matches: &ArgMatches) {
    if let Some(name) = matches.value_of("NAME") {
        let (_user, name) = parse_user_at_name(name);

        vmc.name(name);
    }

    if matches.is_present("names") {
        let names: Vec<&str> = matches.values_of("names").unwrap().collect();
        vmc.names(&names);
    }

    if matches.is_present("parents") {
        let parents: Vec<&str> = matches.values_of("parents").unwrap().collect();
        vmc.parents(&parents);
    }

    if matches.is_present("tags") {
        let tags: Vec<&str> = matches.values_of("tags").unwrap().collect();
        vmc.tags(&tags);
    }

    if matches.is_present("running") {
        vmc.with_pid(WithPid::Filter);
    }
}

fn main() -> Result<()> {
    files::install_main_config()?;
    let matches = cli::build_cli().get_matches();
    let config = Config::new()?;

    if let Some(host) = matches.value_of("host") {
        let args: Vec<String> = args_without_host();
        let mut ssh = Command::new("ssh");
        if matches.subcommand_matches("ssh").is_some() {
            ssh.arg("-t");
        }
        ssh.arg(&host).args(&args);

        ssh.spawn().map_err(|e| Error::executable("ssh", &e.to_string()))?.wait()?;

        return Ok(());
    }

    files::install_all(&config)?;
    let mut vmc = VMsCreator::new(&config);
    if matches.is_present("all-vms") {
        vmc.all();
    }

    if let Some(vm_config) = matches.value_of("vm-config") {
        vmc.vm_config(&fs::read_to_string(&vm_config)?);
    }
    if matches.is_present("minimal-vm-config") {
        vmc.minimal_vm_config();
    }

    match matches.subcommand() {
        Some(("image", image_matches)) => {
            let images_dir = &config.images.directory;
            let other_images_ro_dirs = &config.images.other_directories_ro;
            let mut images_dirs = vec![images_dir];
            images_dirs.extend(other_images_ro_dirs.iter());

            match image_matches.subcommand() {
                Some(("list", _)) => {
                    for image in vml::images::list(&images_dirs)? {
                        println!("{}", image);
                    }
                }

                Some(("available", _)) => {
                    for image in vml::images::available()? {
                        println!("{}", image);
                    }
                }

                Some(("store", store_images_matches)) => {
                    set_specifications(&mut vmc, store_images_matches);
                    vmc.with_pid(WithPid::Without);
                    let image_template = store_images_matches.value_of("image");
                    let force = store_images_matches.is_present("force");

                    for vm in vmc.create()? {
                        let image = if let Some(template) = image_template {
                            template::render(&vm.context(), template, "main: image store (image)")?
                        } else {
                            vm.hyphenized()
                        };
                        vm.store_disk(&config.images.directory.join(&image), force)?;
                    }
                }

                Some(("pull", pull_images_matches)) => {
                    let images = pull_images_matches.values_of("IMAGES").unwrap();

                    for image in images {
                        vml::images::pull(images_dir, image)?;
                    }
                }

                _ => println!("Unexpected images command"),
            }
        }

        Some(("create", create_matches)) => create(&config, &create_matches)?,

        Some(("start", start_matches)) => start(&config, &start_matches, &mut vmc)?,

        Some(("run", run_matches)) => {
            create(&config, &run_matches)?;
            start(&config, &run_matches, &mut vmc)?;
        }

        Some(("stop", stop_matches)) => {
            set_specifications(&mut vmc, stop_matches);

            let force = stop_matches.is_present("force");

            vmc.with_pid(WithPid::Filter);
            vmc.error_on_empty();

            for mut vm in vmc.create()? {
                vm.stop(force)?;
            }
        }

        Some(("ssh", ssh_matches)) => {
            set_specifications(&mut vmc, ssh_matches);

            let mut user = ssh_matches.value_of("user");
            if let Some(name) = ssh_matches.value_of("NAME") {
                let (user_from_arg, _name) = parse_user_at_name(name);
                user = user.or(user_from_arg);
            }

            let ssh_options: Vec<&str> = if ssh_matches.is_present("ssh-options") {
                ssh_matches.values_of("ssh-options").unwrap().collect()
            } else {
                Vec::new()
            };

            let mut ssh_flags: Vec<&str> = Vec::new();
            if ssh_matches.is_present("A") {
                ssh_flags.push("-A");
            }
            if ssh_matches.is_present("Y") {
                ssh_flags.push("-Y");
            }

            let cmd: Option<Vec<&str>> = ssh_matches.values_of("cmd").map(|v| v.collect());

            if vmc.is_all() {
                vmc.with_pid(WithPid::Filter);
            } else {
                vmc.with_pid(WithPid::Error);
            }
            vmc.error_on_empty();
            for vm in vmc.create()? {
                if vm.ssh(&user, &ssh_options, &ssh_flags, &cmd)? != Some(0)
                    && ssh_matches.is_present("check")
                {
                    return Err(Error::SSHFailed(vm.name));
                }
            }
        }

        Some(("rsync-to", rsync_to_matches)) => {
            set_specifications(&mut vmc, rsync_to_matches);

            let mut user = rsync_to_matches.value_of("user");
            if let Some(name) = rsync_to_matches.value_of("NAME") {
                let (user_from_arg, _name) = parse_user_at_name(name);
                user = user.or(user_from_arg);
            }

            let mut rsync_options: Vec<&str> = if rsync_to_matches.is_present("rsync-options") {
                rsync_to_matches.values_of("rsync-options").unwrap().collect()
            } else {
                Vec::new()
            };
            if rsync_to_matches.is_present("archive") {
                rsync_options.push("--archive");
            }
            if rsync_to_matches.is_present("verbose") {
                rsync_options.push("--verbose");
            }
            if rsync_to_matches.is_present("P") {
                rsync_options.push("-P");
            }

            let sources = rsync_to_matches.values_of("sources");
            let template = rsync_to_matches.value_of("template");

            let destination = if rsync_to_matches.is_present("list") {
                None
            } else {
                Some(rsync_to_matches.value_of("destination").unwrap_or("~"))
            };

            if vmc.is_all() {
                vmc.with_pid(WithPid::Filter);
            } else {
                vmc.with_pid(WithPid::Error);
            }
            vmc.error_on_empty();
            if let Some(sources) = sources {
                let sources: Vec<&str> = sources.collect();
                for vm in vmc.create()? {
                    vm.rsync_to(&user, &rsync_options, &sources, &destination)?;
                }
            } else if let Some(template) = template {
                for vm in vmc.create()? {
                    vm.rsync_to_template(&user, &rsync_options, template, &destination)?;
                }
            }
        }

        Some(("rsync-from", rsync_from_matches)) => {
            set_specifications(&mut vmc, rsync_from_matches);

            let mut user = rsync_from_matches.value_of("user");
            if let Some(name) = rsync_from_matches.value_of("NAME") {
                let (user_from_arg, _name) = parse_user_at_name(name);
                user = user.or(user_from_arg);
            }

            let mut rsync_options: Vec<&str> = if rsync_from_matches.is_present("rsync-options") {
                rsync_from_matches.values_of("rsync-options").unwrap().collect()
            } else {
                Vec::new()
            };
            if rsync_from_matches.is_present("archive") {
                rsync_options.push("--archive");
            }
            if rsync_from_matches.is_present("verbose") {
                rsync_options.push("--verbose");
            }
            if rsync_from_matches.is_present("P") {
                rsync_options.push("-P");
            }

            let sources: Vec<&str> = rsync_from_matches.values_of("sources").unwrap().collect();

            let cwd = env::current_dir().unwrap();
            let cwd = cwd.to_string_lossy();
            let destination = if rsync_from_matches.is_present("list") {
                None
            } else {
                Some(rsync_from_matches.value_of("destination").unwrap_or(&cwd))
            };

            if vmc.is_all() {
                vmc.with_pid(WithPid::Filter);
            } else {
                vmc.with_pid(WithPid::Error);
            }
            vmc.error_on_empty();
            for vm in vmc.create()? {
                vm.rsync_from(&user, &rsync_options, &sources, &destination)?;
            }
        }

        Some(("show", show_matches)) => {
            if show_matches.is_present("all") {
                vmc.all();
            }

            set_specifications(&mut vmc, show_matches);

            if show_matches.is_present("running") {
                vmc.with_pid(WithPid::Filter);
            } else {
                vmc.with_pid(WithPid::Option);
            }
            vmc.error_on_empty();

            for vm in vmc.create()? {
                println!("{:#?}", vm);
            }
        }

        Some(("list", list_matches)) => {
            vmc.all();

            if !list_matches.is_present("all") && !config.commands.list.all {
                vmc.with_pid(WithPid::Filter);
            }

            set_specifications(&mut vmc, list_matches);

            list(
                &vmc,
                &config,
                list_matches.is_present("fold"),
                list_matches.is_present("unfold"),
            )?;
        }

        Some(("monitor", monitor_matches)) => {
            set_specifications(&mut vmc, monitor_matches);

            let command = monitor_matches.value_of("command");

            if vmc.is_all() {
                vmc.with_pid(WithPid::Filter);
            } else {
                vmc.with_pid(WithPid::Error);
            }
            vmc.error_on_empty();
            if let Some(command) = command {
                for vm in vmc.create()? {
                    let reply = vm.monitor_command(command)?;
                    if let Some(reply) = reply {
                        println!("{}", reply);
                    }
                }
            } else {
                for vm in vmc.create()? {
                    vm.monitor()?;
                }
            }
        }

        Some(("rm", rm_matches)) => {
            set_specifications(&mut vmc, rm_matches);

            let force = rm_matches.is_present("force");

            if !force {
                vmc.with_pid(WithPid::Without);
            } else {
                vmc.with_pid(WithPid::Option);
            }

            list(&vmc, &config, false, false)?;
            let remove = confirm("Do you really want to remove that vms?");

            if remove {
                for mut vm in vmc.create()? {
                    if force && vm.has_pid() {
                        vm.stop(true)?;
                    }

                    vm.remove()?;
                }
            }
        }

        Some(("completion", completion_matches)) => {
            cli::completion(completion_matches.value_of("SHELL").unwrap())
        }

        _ => println!("Unexpected command"),
    }

    Ok(())
}
