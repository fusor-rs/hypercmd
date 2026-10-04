# Build terminal apps with HTML and Rust

Hypercmd renders [Fusor](https://fusor.build/docs/) templates in terminal cells. Write
views in HTML and keep application state and behavior in Rust. Fusor provides the template
compiler and reactive state: values that update the interface when they change.

## Get started

Follow the [installation and first application walkthrough](../README.md#get-started).
The [CLI reference](cli.md) covers creating, checking, running and building apps.
The [HTML and CSS reference](profile.md) describes the supported HTML and CSS, and
[controls](controls.md) covers focus, keyboard input, and form bindings.

## Build an application

Once the first app runs, these pages cover the main areas:

- Use [routing](routing.md) to navigate between terminal screens.
- Load data with [async views](async.md) and owner-scoped [services](services.md).
- Read [native execution](native.md) for terminal sessions and resource limits.
- Package [shared components](consumption.md) for terminal and browser consumers.

## Status

Hypercmd is experimental. The [status and limitations](../README.md#status-and-known-limitations)
describe the current platform support and authoring profile.
