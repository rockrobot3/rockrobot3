<p align="center">
  <img src=".github/logo/logo.png" width="100%" alt="Rockrobot 3 logo">
</p>

<h1 align="center">Rockrobot 3 洛克三号</h1>

<p align="center">
  <a href="https://atomgit.com/rockrobot/rockrobot3"><img src="https://atomgit.com/rockrobot/rockrobot3/star/badge.svg" alt="AtomGit"></a>
  <a href="https://github.com/rockrobot3/rockrobot3"><img src="https://img.shields.io/github/stars/rockrobot3/rockrobot3?style=flat&logo=github&label=stars" alt="GitHub stars"></a>
  <a href="https://github.com/rockrobot3/rockrobot3/releases/latest"><img src="https://img.shields.io/github/v/release/rockrobot3/rockrobot3?label=version&color=5a6b7e" alt="version"></a>
  <img src="https://img.shields.io/badge/Rust-1.100.0-DEA584?logo=rust&logoColor=white" alt="Rust 1.100.0">
  <a href="https://atomgit.com/openairymax"><img src="https://img.shields.io/badge/OpenAirymax-DA203E" alt="OpenAirymax"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-4a90d9?logo=apache&logoColor=white" alt="license Apache-2.0"></a>
</p>

## Introduction

Rockrobot-3 is a robot system base.

Swapping the chip on the same robot body enables cross-chip-platform deployment in as little as **3 days**, with no code changes.

## Architecture

Rockrobot 3 also includes the robot body built on this base. The base provides process management, inter-process communication, hardware abstraction, and a module framework, written in Rust; on top of it, the body implements voice sensing, motion execution, expressive interaction, power management, network connectivity, and dialog orchestration, with dialogs driven by cloud-based speech recognition, large language models, and text-to-speech services. The same codebase supports two categories of target platforms in architecture: Linux single-board computers (Raspberry Pi, RK3566, etc.) and ESP32 microcontrollers. Switching platforms requires no software rewrite.

- **Cross-platform**: The program is divided into two layers according to whether it depends on the operating system. Message formats and control logic are OS-independent; threads, networking, and drivers are platform-specific. When moving to a new platform, the former remains unchanged, while the latter is re-implemented behind a uniform interface, along with a configuration profile for that model.
- **Modular**: Functions are divided into six modules. Modules never call each other directly; all communication goes through a message bus. Which messages each module may publish and subscribe to is defined by the contract files (`contracts/`); contracts only accept additions and never modify existing definitions, keeping old and new versions compatible.
- **Configuration separate from code**: Chip model, pin assignment, enabled modules, and other hardware details live in configuration files (`profiles/`, `hardware/`). Changing hardware or model is done by editing configuration, with no changes to program code.
- **Validation before startup**: At startup, the system checks contracts, model configuration, and wiring item by item; if any item is not satisfied, startup is rejected (fail-closed) — the system never runs on incomplete configuration.

## Functional Modules

The body consists of the following six modules:

| Module | Responsibility |
|---|---|
| rr3-sense | Voice capture and voice activity detection (VAD) |
| rr3-act | Servo, lighting, and relay control; the real-time control loop runs locally, without going through the message bus |
| rr3-interact | Expression rendering, audio playback, local panel |
| rr3-power | Battery, charging, and power management; low-battery handling executes locally |
| rr3-connect | Wi-Fi and network connectivity |
| rr3-brain | Dialog flow orchestration, chaining speech recognition, large language models, and text-to-speech (cloud services) |

## Repository Layout

```
rockrobot3/
├── contracts/    # Shared contracts: message formats, topic registry, capability definitions; append-only, for version compatibility
├── base/
│   ├── runtime/  # Base runtime: message bus, core state machine, daemons, etc.
│   ├── os/       # System image build (deb packages and Buildroot images)
│   └── ota/      # Over-the-air updates
├── modules/      # Robot functional modules (see the table above)
├── profiles/     # Model configuration: chip, peripherals, pins, enabled modules
├── hardware/     # Wiring configuration: device-to-pin mapping
├── tools/        # Check scripts, module generator, simulation tools (see tools/README.md)
└── docs/         # Architecture, development, and status documentation
```

How the two layers map to directories: the base lives in `base/`, body features in `modules/`, shared contracts in `contracts/`, and model and hardware information in `profiles/` and `hardware/`. Bringing up a new model touches only `profiles/`, `hardware/`, and the platform interface adaptation.

## Getting Started

```bash
git clone https://github.com/rockrobot3/rockrobot3.git
cd rockrobot3
bash tools/ci/check.sh
```

`check.sh` runs the repository's full checks, matching what CI runs; a Bash environment is required.

- Generate a new module skeleton: `tools/mkmodule`
- Hardware-free simulation: `tools/sim`
- Runtime interface reference (startup arguments, RPC, registration rules): [docs/reference/](docs/reference/)

## Contributing

Discussions and contributions are welcome via Issues.

Chinese edition: [README_zh.md](README_zh.md).

## License

This project is licensed under the [Apache License 2.0](LICENSE).
