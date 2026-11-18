# hxn-gpu-gles

[![Ash](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/ash.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/ash.yml)
[![HashLink](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/hashlink.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/hashlink.yml)
[![hxcpp](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/hxcpp.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/hxcpp.yml)
[![CPPIA](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/cppia.yml/badge.svg)](https://github.com/hx-n/hxn-gpu-gles/actions/workflows/cppia.yml)

OpenGL ES 3.0 bindings for Haxe on Ash, HashLink, hxcpp and CPPIA, over EGL.
It targets Android devices without Vulkan, embedded Linux, and desktops
through Mesa or [ANGLE](https://chromium.googlesource.com/angle/angle). The
API follows OpenGL ES: an EGL `Display` and `Context`, buffers, textures,
samplers, programs, vertex arrays and framebuffers, bound by texture unit and
uniform buffer binding point. OpenGL ES orders commands itself, so there are
no barriers; you wait only to read results on the CPU.

Shaders come from [hxnsl](https://github.com/hx-n/hxnsl), which generates GLSL
ES 3.00 at compile time and lists each stage's bindings in `GLSL_BINDINGS`.
`Bindings` assigns the binding points and texture units from it, so no
program is queried for its resources at run time.

```haxe
import hxn.gpu.gles.*;

var display = Display.headless();
var context = display.createContext();                    // current on this thread
var bindings = new Bindings(Tinted.GLSL_BINDINGS);         // an hxnsl shader
var program = hxn.gpu.gles.Shaders.program(context, Tinted.GLSL_VERTEX, Tinted.GLSL_FRAGMENT, bindings);
var vertices = context.createVertexArray();
vertices.attribute(Tinted.INPUT_position, positions, 2, ComponentType.Float, false, 8, 0);
var target = context.createTexture(width, height, TextureFormat.Rgba8);
var framebuffer = context.createFramebuffer(target);

context.bindFramebuffer(framebuffer);
context.viewport(0, 0, width, height);
context.clear(ClearBits.Color, 0, 0, 0, 1);
context.useProgram(program);
context.bindVertexArray(vertices);
context.bindUniformBuffer(bindings.uniformBlock(Tinted.PARAMS_GROUP, Tinted.PARAMS_BINDING), uniforms);
context.drawArrays(Primitive.TriangleStrip, 0, 4, 1);
context.completed().then(_ -> trace(context.readPixels(0, 0, width, height)));
```

[`examples/check/Main.hx`](https://github.com/hx-n/hxn-gpu-gles/blob/main/examples/check/Main.hx)
is the whole program: a render pass read back once the GPU has finished, a
pass sampling a texture, and a buffer read back through a mapping.

## Installing

```
haxelib install hxn-gpu-gles
haxelib install hxnsl
haxe -lib hxn-gpu-gles -lib hxnsl -main Main -hl main.hl
haxe -lib hxn-gpu-gles -lib hxnsl -main Main -cpp out
```

The haxelib carries the native library prebuilt for Linux on x86-64 and
ARM64, Windows on x86-64 and macOS on Apple silicon, for HashLink and hxcpp,
and as static libraries for hxcpp on Intel Macs and for Android. A program
needs no Rust.

The library loads EGL and OpenGL ES when a display opens, and links against
neither. On Linux they are Mesa's (`libegl1 libgles2 libegl-mesa0`), and on
Android the system's. macOS and Windows have no EGL of their own: point
`HXN_GPU_GLES_EGL` at ANGLE's `libEGL`, and the library loads ANGLE's
`libGLESv2` from beside it. Electron's releases up to 43 include both, beside
the Electron framework on macOS and `electron.exe` on Windows; Chrome and
later Electron link ANGLE in. `HXN_GPU_GLES_GLES` names OpenGL ES's library
when it is elsewhere.

## Classes

All in the package `hxn.gpu.gles`:

| Class | |
|---|---|
| `Display` | `headless()`, Mesa's surfaceless platform where EGL has it, or `native(platform, display)` for a window system's display; `createContext` and `createWindowContext(window, visual)`. |
| `Context` | Object creation, framebuffer and program binding, draws, `readPixels`, `swapBuffers`, `finish` and `completed`. |
| `Buffer` | `write`, and `read` through a mapping. |
| `Texture` | A 2D texture of one level with immutable storage, `write` by rectangle. |
| `Sampler` | Filters and wrap modes, bound to a texture unit with a texture. |
| `Program` | A linked program; `uniformBlock(name, binding)` and `sampler(name, unit)`. |
| `VertexArray` | `attribute`, `integerAttribute`, `divisor` and `elementBuffer`. |
| `Framebuffer` | A framebuffer rendering into one texture. |
| `Bindings` | The binding points and texture units of an hxnsl shader. |

Every object holds its OpenGL ES object until `close()`, and the context
lives until everything made from it is closed. Errors making objects, and
compile and link errors with OpenGL ES's log, are thrown as Haxe exceptions;
`Context.error()` is `glGetError`.

## Windows

`Display.native(platform, display)` opens the window system's display, and
`createWindowContext(window, visual)` makes a context whose default
framebuffer is the window; `bindDefaultFramebuffer` renders into it and
`swapBuffers` presents it. With hxn-window on Xlib:

```haxe
var display = Display.native(0x31D5, window.raw(2));    // EGL_PLATFORM_X11_KHR, the Display*
var context = display.createWindowContext(window.raw(0), haxe.Int64.toInt(window.raw(1)));
```

`visual` picks the EGL config of the window's X11 visual; 0 takes any. The
other window systems take `Display.native(0, 0)`, the default display, and
`createWindowContext(window.raw(0), 0)`:

- Android: `window` is the `ANativeWindow*`.
- Windows through ANGLE: `window` is the `HWND`.
- macOS through ANGLE: `window` is the `NSView`, which gets a `CAMetalLayer`
  that ANGLE renders into.

Wayland takes `Display.native(0x31D8, window.raw(2))`
(`EGL_PLATFORM_WAYLAND_KHR`, the `wl_display`) and
`createWaylandContext(window.raw(0), width, height)`, which makes the
`wl_egl_window` EGL renders the `wl_surface` through, from libwayland-egl.
`Context.resizeWindow` follows the window's size there; on the other window
systems the window sets it.
[`examples/present/Main.hx`](https://github.com/hx-n/hxn-gpu-gles/blob/main/examples/present/Main.hx)
presents to an X11 window, and CI runs it under Xvfb.

## Threads

A context is current on one thread at a time, and its objects are used on
that thread. `createContext` and `createWindowContext` make the new context
current on the calling thread, and `makeCurrent` moves it. An object closed
or collected while its context is not current is deleted at the context's
next call.

`Context.completed()` returns an `hxn.Future` that settles when the GPU has
finished every command so far. With EGL 1.5 and `EGL_KHR_fence_sync`, a thread
waits on an EGL fence; otherwise it calls `glFinish` and the future is settled
already. `finish()` is the blocking form.

## Shaders from hxnsl

A shader for this driver implements `hxn.gpu.gles.Shader`, which extends
`hxnsl.Shader` and gets hxnsl's `GLSL_VERTEX`, `GLSL_FRAGMENT` and
`GLSL_BINDINGS`. A compute shader, or one with storage buffers, has
`GLSL_MISSING` instead, since OpenGL ES 3.0 cannot run it.

- `new Bindings(Shader.GLSL_BINDINGS)` numbers a binding point for each
  uniform block and a texture unit for each texture with its sampler. It
  gives them by hxnsl's group and binding: `uniformBlock(group, binding)` and
  `textureUnit(group, binding)`.
- `hxn.gpu.gles.Shaders.program(context, vertex, fragment, bindings)` links the
  program and assigns them to it by the names hxnsl gives each stage.
- Vertex inputs have explicit locations, which are hxnsl's `INPUT_` constants.

hxnsl's GLSL flips Y in the vertex stage, so images come out the way WebGPU
draws them with an ordinary viewport.

## Building from source

Requires the [hxn](https://github.com/hx-n/hxn) CLI, Rust, Haxe 4.3, hxnsl,
and EGL with OpenGL ES 3.0. `hxn check` builds the binding and runs
`examples/check` on each runtime named, and fails unless every run passes and
prints the same output. CI runs it on Linux with Mesa's llvmpipe, and on
macOS and Windows with ANGLE from Electron 43.8.3:

```
hxn check hl --vm ash
hxn check cpp cppia
```

## Agent skill

[`skills/hxn-gpu-gles/SKILL.md`](https://github.com/hx-n/hxn-gpu-gles/blob/main/skills/hxn-gpu-gles/SKILL.md)
describes the library for coding agents. The haxelib includes it.

## License

Apache-2.0.

## Third-party notices

Dependency licenses and redistribution notices are in
[third-party/NOTICES.md](https://github.com/hx-n/hxn-gpu-gles/blob/main/third-party/NOTICES.md). Retain the applicable
notices when redistributing binaries.
