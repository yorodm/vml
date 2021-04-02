# vml
VML is a tool for easily and transparently work with qemu virtual machines.
Virtual machines present as directories with vml.toml files in it.
VML is able to initialize images with cloud-init. Virtual machines with ALT,
Centos, Debian and Ubuntu could be created with just one command.

## Build
All needed dependencies saved into vendor directory, so it can be built
in the offline mode.
```
cargo build --release --offline
```

Or just download release binary.
```
wget https://github.com/Obirvalger/vml/releases/download/v0.1.0/vml
```

Then copy created executable to appropriate path, e.g. `~/bin/vml`, if
`~/bin` is in your PATH.
```
cp target/release/vml ~/bin/vml
```

The following programs (packages) should be installed:
* `kvm` (qemu-kvm)
* `rsync`
* `socat`
* `cloud-localds` (cloud-utils)

User running `vml` should be able to use kvm (.e.g be in `vmusers` group).

## Run
All needed files are copied with any command. For example list available to
pull images.
```
vml images available
```

Or get completion for your shell (zsh in example).
```
vml completion zsh
```

Then edit config with your preferences.
```
$EDITOR ~/.config/vml/config.toml
```

Run vm named `test`, using `alt-sisyphus` image.
```
vml run -i alt-sisyphus --wait-ssh -n test
```

VM `test` is described via directory `test` in `<vms-dir>` (vms-dir from
config) and within files: `test.qcow` is a disk image, `vml.tml` is a current
vm config file. By default `vml.toml` is empty, but it is needed to mark the
directory as `vml` vm. Some fields of the `vml.toml` have names as `default`
section fields of the main config file `~/.config/vml/config.toml`.

Finally ssh to the `test` vm.
```
vml ssh -n test
```
