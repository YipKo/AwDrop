# AwDrop

AwDrop is an experimental cross-platform project for AirDrop connectivity,
built on [winawdl](https://github.com/raph559/winawdl) and its upstream projects.
The current work includes a hardware-neutral radio adapter interface and desktop
application changes. Windows is the current development target; Linux support
and additional radio backends are planned.

The MT7922 backend is not yet implemented. Identifying a Wi-Fi card does not
mean that it is supported. Do not install a driver for a different device.
See [the adapter documentation](winawdl/adapter/README.md) and the
[upstream documentation](winawdl/README.md) for the current limitations.

After cloning, initialize the upstream source dependencies:

```sh
git submodule update --init
git -C winawdl/upstream/owl submodule update --init radiotap
```

The project contains components under different licenses. See [LICENSE](LICENSE)
and the component licenses before distributing source or binaries. AwDrop is an
independent project and is not affiliated with or endorsed by Apple.
