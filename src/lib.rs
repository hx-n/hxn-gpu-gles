//! OpenGL ES 3.0 for Haxe over EGL: a thin layer keeping OpenGL ES's conventions. A program
//! makes a context, creates objects through it, and binds them by unit and binding point as
//! OpenGL ES does. Shaders are hxnsl's GLSL ES 3.00, bound by the names hxnsl reports in
//! `GLSL_BINDINGS`. EGL and OpenGL ES are loaded when a display opens, so the library links
//! against neither.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use glow::HasContext;
use hxn_future::Future;
use khronos_egl as egl;

hxn::package!("gpu.gles");

type Egl = egl::DynamicInstance<egl::EGL1_4>;

/// An EGL or OpenGL ES object. EGL objects are usable from any thread; OpenGL ES objects are
/// used on the thread where their context is current, which `Context.makeCurrent` sets.
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

fn egl_failed(what: &str, error: egl::Error) -> String {
    format!("{what}: {error}")
}

fn u32_of(value: i32, what: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{what} cannot be {value}"))
}

fn i32_of(value: i64, what: &str) -> Result<i32, String> {
    i32::try_from(value).map_err(|_| format!("{what} cannot be {value}"))
}

/// The libraries EGL and OpenGL ES are loaded from on each platform, unless
/// `HXN_GPU_GLES_EGL` and `HXN_GPU_GLES_GLES` name others.
fn library_names() -> (&'static str, &'static str) {
    if cfg!(target_os = "android") {
        ("libEGL.so", "libGLESv2.so")
    } else if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        ("libEGL.dylib", "libGLESv2.dylib")
    } else if cfg!(windows) {
        ("libEGL.dll", "libGLESv2.dll")
    } else {
        ("libEGL.so.1", "libGLESv2.so.2")
    }
}

/// A texture's internal format, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy, PartialEq)]
pub enum TextureFormat {
    R8 = 0x8229,
    Rg8 = 0x822B,
    Rgba8 = 0x8058,
    Srgb8Alpha8 = 0x8C43,
    R32f = 0x822E,
    Rgba16f = 0x881A,
    Rgba32f = 0x8814,
    Depth24Stencil8 = 0x88F0,
    Depth32f = 0x8CAC,
}

impl TextureFormat {
    /// The format and type `texSubImage2D` takes for it.
    fn transfer(self) -> (u32, u32) {
        match self {
            TextureFormat::R8 => (glow::RED, glow::UNSIGNED_BYTE),
            TextureFormat::Rg8 => (glow::RG, glow::UNSIGNED_BYTE),
            TextureFormat::Rgba8 | TextureFormat::Srgb8Alpha8 => (glow::RGBA, glow::UNSIGNED_BYTE),
            TextureFormat::R32f => (glow::RED, glow::FLOAT),
            TextureFormat::Rgba16f => (glow::RGBA, glow::HALF_FLOAT),
            TextureFormat::Rgba32f => (glow::RGBA, glow::FLOAT),
            TextureFormat::Depth24Stencil8 => (glow::DEPTH_STENCIL, glow::UNSIGNED_INT_24_8),
            TextureFormat::Depth32f => (glow::DEPTH_COMPONENT, glow::FLOAT),
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self {
            TextureFormat::R8 => 1,
            TextureFormat::Rg8 => 2,
            TextureFormat::Rgba16f => 8,
            TextureFormat::Rgba32f => 16,
            _ => 4,
        }
    }

    fn attachment(self) -> u32 {
        match self {
            TextureFormat::Depth24Stencil8 => glow::DEPTH_STENCIL_ATTACHMENT,
            TextureFormat::Depth32f => glow::DEPTH_ATTACHMENT,
            _ => glow::COLOR_ATTACHMENT0,
        }
    }
}

/// The binding target a buffer is created for, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum BufferTarget {
    Array = 0x8892,
    ElementArray = 0x8893,
    Uniform = 0x8A11,
    CopyRead = 0x8F36,
    CopyWrite = 0x8F37,
    PixelPack = 0x88EB,
    PixelUnpack = 0x88EC,
}

/// A buffer's usage hint, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum Usage {
    StaticDraw = 0x88E4,
    DynamicDraw = 0x88E8,
    StreamDraw = 0x88E0,
    StaticRead = 0x88E5,
    DynamicRead = 0x88E9,
    StreamRead = 0x88E1,
}

/// A sampler's filter, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum Filter {
    Nearest = 0x2600,
    Linear = 0x2601,
}

/// A sampler's wrap mode, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum Wrap {
    Repeat = 0x2901,
    ClampToEdge = 0x812F,
    MirroredRepeat = 0x8370,
}

/// A draw's primitive, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum Primitive {
    Points = 0,
    Lines = 1,
    LineLoop = 2,
    LineStrip = 3,
    Triangles = 4,
    TriangleStrip = 5,
    TriangleFan = 6,
}

/// A vertex attribute's or index's component type, OpenGL ES's values.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum ComponentType {
    Byte = 0x1400,
    UnsignedByte = 0x1401,
    Short = 0x1402,
    UnsignedShort = 0x1403,
    Int = 0x1404,
    UnsignedInt = 0x1405,
    Float = 0x1406,
    HalfFloat = 0x140B,
}

/// `Context.clear` bits, OpenGL ES's values, or-ed together.
#[hxn::data]
#[derive(Clone, Copy)]
pub enum ClearBits {
    Depth = 0x100,
    Stencil = 0x400,
    Color = 0x4000,
}

/// An OpenGL ES object to delete, deferred while its context is not current on the thread
/// that drops it.
enum Name {
    Buffer(glow::Buffer),
    Texture(glow::Texture),
    Sampler(glow::Sampler),
    Program(glow::Program),
    VertexArray(glow::VertexArray),
    Framebuffer(glow::Framebuffer),
}

struct DisplayInner {
    egl: Shared<Egl>,
    display: Shared<egl::Display>,
    /// EGL 1.5 with fence syncs, which `Context.completed` waits on from another thread.
    fences: bool,
    /// OpenGL ES's library, for the functions `eglGetProcAddress` does not return.
    gles: Option<libloading::Library>,
}

impl Drop for DisplayInner {
    fn drop(&mut self) {
        let _ = self.egl.0.terminate(self.display.0);
    }
}

/// An EGL display: the connection contexts are made on.
#[hxn::interface]
pub struct Display {
    inner: Arc<DisplayInner>,
}

fn load() -> Result<(Egl, Option<libloading::Library>), String> {
    let (egl_name, gles_name) = library_names();
    let egl_path = std::env::var("HXN_GPU_GLES_EGL").unwrap_or_else(|_| egl_name.to_string());
    // OpenGL ES beside EGL when EGL came from a path, else by its usual name.
    let gles_path = std::env::var("HXN_GPU_GLES_GLES").unwrap_or_else(|_| {
        match std::path::Path::new(&egl_path).parent() {
            Some(dir) if !dir.as_os_str().is_empty() => {
                dir.join(gles_name).to_string_lossy().into_owned()
            }
            _ => gles_name.to_string(),
        }
    });
    // Loaded first: ANGLE's EGL imports OpenGL ES's library by name, which Windows resolves
    // from the program's directory and PATH but takes from the libraries already loaded.
    let gles = unsafe { libloading::Library::new(&gles_path) }.ok();
    let egl = unsafe { Egl::load_required_from_filename(&egl_path) }
        .map_err(|e| format!("loading EGL from {egl_path}: {e}"))?;
    Ok((egl, gles))
}

fn open(
    egl: Egl,
    gles: Option<libloading::Library>,
    display: egl::Display,
) -> Result<Display, String> {
    let (major, minor) = egl
        .initialize(display)
        .map_err(|e| egl_failed("initializing the EGL display", e))?;
    let fences = (major, minor) >= (1, 5)
        && egl.upcast::<egl::EGL1_5>().is_some()
        && egl
            .query_string(Some(display), egl::EXTENSIONS)
            .is_ok_and(|e| {
                e.to_string_lossy()
                    .split(' ')
                    .any(|e| e == "EGL_KHR_fence_sync")
            });
    Ok(Display {
        inner: Arc::new(DisplayInner {
            egl: Shared(egl),
            display: Shared(display),
            fences,
            gles,
        }),
    })
}

fn client_extensions(egl: &Egl) -> String {
    egl.query_string(None, egl::EXTENSIONS)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `EGL_PLATFORM_SURFACELESS_MESA`.
const PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;

#[hxn::methods]
impl Display {
    /// A display that needs no window system: Mesa's surfaceless platform where EGL has it,
    /// else the default display.
    pub fn headless() -> Result<Display, String> {
        let (egl, gles) = load()?;
        let surfaceless = client_extensions(&egl)
            .split(' ')
            .any(|e| e == "EGL_MESA_platform_surfaceless");
        let display = match egl.upcast::<egl::EGL1_5>() {
            Some(egl15) if surfaceless => unsafe {
                egl15.get_platform_display(
                    PLATFORM_SURFACELESS_MESA,
                    egl::DEFAULT_DISPLAY,
                    &[egl::ATTRIB_NONE],
                )
            }
            .map_err(|e| egl_failed("opening the surfaceless display", e))?,
            _ => unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }
                .ok_or("EGL has no default display")?,
        };
        open(egl, gles, display)
    }

    /// The display of a window system: `platform` is an EGL platform, such as
    /// `EGL_PLATFORM_X11_KHR` (0x31D5) or `EGL_PLATFORM_WAYLAND_KHR` (0x31D8), and `native`
    /// that platform's display, which hxn-window's `raw` gives. Platform 0 takes `native` as
    /// an `EGLNativeDisplayType`, and 0 there is the default display.
    pub fn native(platform: i32, native: i64) -> Result<Display, String> {
        let (egl, gles) = load()?;
        let pointer = native as usize as *mut c_void;
        let display = if platform == 0 {
            unsafe { egl.get_display(pointer) }.ok_or("EGL has no such display")?
        } else {
            let egl15 = egl
                .upcast::<egl::EGL1_5>()
                .ok_or("a platform display needs EGL 1.5")?;
            unsafe {
                egl15.get_platform_display(platform as egl::Enum, pointer, &[egl::ATTRIB_NONE])
            }
            .map_err(|e| egl_failed("opening the platform display", e))?
        };
        open(egl, gles, display)
    }

    /// EGL's vendor string.
    pub fn vendor(&self) -> String {
        self.query(egl::VENDOR)
    }

    /// EGL's version string.
    pub fn version(&self) -> String {
        self.query(egl::VERSION)
    }

    /// The display's EGL extensions, separated by spaces.
    pub fn extensions(&self) -> String {
        self.query(egl::EXTENSIONS)
    }

    /// An OpenGL ES 3 context with no default framebuffer: it renders into framebuffers alone.
    /// It is current on the calling thread.
    pub fn create_context(&self) -> Result<Context, String> {
        let surfaceless = self
            .extensions()
            .split(' ')
            .any(|e| e == "EGL_KHR_surfaceless_context");
        let surface_type = if surfaceless { 0 } else { egl::PBUFFER_BIT };
        let config = self.config(surface_type)?;
        let surface = if surfaceless {
            None
        } else {
            let attributes = [egl::WIDTH, 1, egl::HEIGHT, 1, egl::NONE];
            Some(
                self.inner
                    .egl
                    .0
                    .create_pbuffer_surface(self.inner.display.0, config, &attributes)
                    .map_err(|e| egl_failed("creating a pbuffer surface", e))?,
            )
        };
        self.context(config, surface)
    }

    /// An OpenGL ES 3 context whose default framebuffer is the window `window`, an
    /// `EGLNativeWindowType`: an `ANativeWindow*` on Android, an X11 window on Xlib. `visual`
    /// is the window's X11 visual id, which picks a config of that visual; 0 takes any. With
    /// hxn-window on Xlib they are `raw(0)` and `raw(1)`. It is current on the calling
    /// thread.
    pub fn create_window_context(&self, window: i64, visual: i32) -> Result<Context, String> {
        let config = if visual == 0 {
            self.config(egl::WINDOW_BIT)?
        } else {
            self.config_of_visual(visual)?
        };
        let surface = unsafe {
            self.inner.egl.0.create_window_surface(
                self.inner.display.0,
                config,
                window as usize as egl::NativeWindowType,
                None,
            )
        }
        .map_err(|e| egl_failed("creating the window surface", e))?;
        self.context(config, Some(surface))
    }
}

impl Display {
    fn query(&self, name: egl::Int) -> String {
        self.inner
            .egl
            .0
            .query_string(Some(self.inner.display.0), name)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn config(&self, surface_type: egl::Int) -> Result<egl::Config, String> {
        self.inner
            .egl
            .0
            .choose_first_config(self.inner.display.0, &Self::attributes(surface_type))
            .map_err(|e| egl_failed("choosing an EGL config", e))?
            .ok_or_else(|| "EGL has no OpenGL ES 3 config with 8-bit RGBA".to_string())
    }

    /// A window config whose native visual is `visual`, with 8-bit color and any alpha.
    fn config_of_visual(&self, visual: i32) -> Result<egl::Config, String> {
        let egl = &self.inner.egl.0;
        let display = self.inner.display.0;
        let mut attributes = Self::attributes(egl::WINDOW_BIT);
        attributes[7] = 0;
        let count = egl
            .get_config_count(display)
            .map_err(|e| egl_failed("counting EGL configs", e))?;
        let mut configs = Vec::with_capacity(count);
        egl.choose_config(display, &attributes, &mut configs)
            .map_err(|e| egl_failed("choosing an EGL config", e))?;
        configs
            .into_iter()
            .find(|&c| egl.get_config_attrib(display, c, egl::NATIVE_VISUAL_ID) == Ok(visual))
            .ok_or_else(|| format!("EGL has no OpenGL ES 3 config of visual 0x{visual:X}"))
    }

    /// Config attributes for OpenGL ES 3 with 8-bit RGBA; index 7 is the alpha size.
    fn attributes(surface_type: egl::Int) -> [egl::Int; 13] {
        [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES3_BIT,
            egl::SURFACE_TYPE,
            surface_type,
            egl::NONE,
        ]
    }

    fn context(
        &self,
        config: egl::Config,
        surface: Option<egl::Surface>,
    ) -> Result<Context, String> {
        let egl = &self.inner.egl.0;
        let display = self.inner.display.0;
        egl.bind_api(egl::OPENGL_ES_API)
            .map_err(|e| egl_failed("binding OpenGL ES", e))?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            )
            .map_err(|e| egl_failed("creating the OpenGL ES 3 context", e))?;
        egl.make_current(display, surface, surface, Some(context))
            .map_err(|e| egl_failed("making the context current", e))?;
        let gles = &self.inner.gles;
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                if let Some(f) = egl.get_proc_address(name) {
                    return f as *const c_void;
                }
                gles.as_ref()
                    .and_then(|lib| lib.get::<*const c_void>(name.as_bytes()).ok().map(|s| *s))
                    .unwrap_or(std::ptr::null())
            })
        };
        Ok(Context {
            gl: Arc::new(Gl {
                display: self.inner.clone(),
                context: Shared(context),
                surface: Shared(surface),
                gl: Shared(gl),
                pending: Mutex::new(Vec::new()),
                has_pending: AtomicBool::new(false),
            }),
        })
    }
}

struct Gl {
    display: Arc<DisplayInner>,
    context: Shared<egl::Context>,
    surface: Shared<Option<egl::Surface>>,
    gl: Shared<glow::Context>,
    pending: Mutex<Vec<Name>>,
    has_pending: AtomicBool,
}

impl Gl {
    fn is_current(&self) -> bool {
        self.display.egl.0.get_current_context() == Some(self.context.0)
    }

    /// Deletes `name` now if the context is current on this thread, else at its next call.
    fn release(&self, name: Name) {
        if self.is_current() {
            self.delete(name);
        } else {
            self.pending.lock().unwrap().push(name);
            self.has_pending.store(true, Ordering::Release);
        }
    }

    fn delete(&self, name: Name) {
        let gl = &self.gl.0;
        unsafe {
            match name {
                Name::Buffer(n) => gl.delete_buffer(n),
                Name::Texture(n) => gl.delete_texture(n),
                Name::Sampler(n) => gl.delete_sampler(n),
                Name::Program(n) => gl.delete_program(n),
                Name::VertexArray(n) => gl.delete_vertex_array(n),
                Name::Framebuffer(n) => gl.delete_framebuffer(n),
            }
        }
    }

    /// The OpenGL ES functions, after deleting what was dropped while the context was not
    /// current.
    fn gl(&self) -> &glow::Context {
        if self.has_pending.load(Ordering::Acquire) {
            let names = std::mem::take(&mut *self.pending.lock().unwrap());
            self.has_pending.store(false, Ordering::Release);
            for name in names {
                self.delete(name);
            }
        }
        &self.gl.0
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        let egl = &self.display.egl.0;
        let display = self.display.display.0;
        if self.is_current() {
            for name in std::mem::take(&mut *self.pending.lock().unwrap()) {
                self.delete(name);
            }
            let _ = egl.make_current(display, None, None, None);
        }
        if let Some(surface) = self.surface.0 {
            let _ = egl.destroy_surface(display, surface);
        }
        let _ = egl.destroy_context(display, self.context.0);
    }
}

/// An OpenGL ES 3 context. Its methods are OpenGL ES's, called on the thread where it is
/// current.
#[hxn::interface]
pub struct Context {
    gl: Arc<Gl>,
}

#[hxn::methods]
impl Context {
    /// Makes the context current on the calling thread.
    pub fn make_current(&self) -> Result<(), String> {
        let display = &self.gl.display;
        let surface = self.gl.surface.0;
        display
            .egl
            .0
            .make_current(display.display.0, surface, surface, Some(self.gl.context.0))
            .map_err(|e| egl_failed("making the context current", e))
    }

    /// Presents the window's back buffer, for a context made with `createWindowContext`.
    pub fn swap_buffers(&self) -> Result<(), String> {
        let display = &self.gl.display;
        let surface = self.gl.surface.0.ok_or("the context has no window")?;
        display
            .egl
            .0
            .swap_buffers(display.display.0, surface)
            .map_err(|e| egl_failed("swapping buffers", e))
    }

    /// The number of vertical blanks `swapBuffers` waits for: 0 presents at once.
    pub fn swap_interval(&self, interval: i32) -> Result<(), String> {
        let display = &self.gl.display;
        display
            .egl
            .0
            .swap_interval(display.display.0, interval)
            .map_err(|e| egl_failed("setting the swap interval", e))
    }

    /// `GL_RENDERER`.
    pub fn renderer(&self) -> String {
        unsafe { self.gl.gl().get_parameter_string(glow::RENDERER) }
    }

    /// `GL_VERSION`.
    pub fn version(&self) -> String {
        unsafe { self.gl.gl().get_parameter_string(glow::VERSION) }
    }

    /// `GL_EXTENSIONS`, separated by spaces.
    pub fn extensions(&self) -> String {
        let gl = self.gl.gl();
        gl.supported_extensions()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `glGetError`: 0, or the first error since the last call.
    pub fn error(&self) -> i32 {
        unsafe { self.gl.gl().get_error() as i32 }
    }

    /// A buffer of `size` bytes, uninitialized, for `target`.
    pub fn create_buffer(
        &self,
        target: BufferTarget,
        size: i64,
        usage: Usage,
    ) -> Result<Buffer, String> {
        let gl = self.gl.gl();
        let size = i32_of(size, "a buffer's size")?;
        let name = unsafe { gl.create_buffer() }?;
        unsafe {
            gl.bind_buffer(target as u32, Some(name));
            gl.buffer_data_size(target as u32, size, usage as u32);
            gl.bind_buffer(target as u32, None);
        }
        Ok(Buffer {
            gl: self.gl.clone(),
            name,
            target: target as u32,
            size,
        })
    }

    /// A buffer holding `bytes`, for `target`.
    pub fn create_buffer_with(
        &self,
        target: BufferTarget,
        bytes: &[u8],
        usage: Usage,
    ) -> Result<Buffer, String> {
        let gl = self.gl.gl();
        let size = i32_of(bytes.len() as i64, "a buffer's size")?;
        let name = unsafe { gl.create_buffer() }?;
        unsafe {
            gl.bind_buffer(target as u32, Some(name));
            gl.buffer_data_u8_slice(target as u32, bytes, usage as u32);
            gl.bind_buffer(target as u32, None);
        }
        Ok(Buffer {
            gl: self.gl.clone(),
            name,
            target: target as u32,
            size,
        })
    }

    /// A 2D texture of one level, with immutable storage.
    pub fn create_texture(
        &self,
        width: i32,
        height: i32,
        format: TextureFormat,
    ) -> Result<Texture, String> {
        let gl = self.gl.gl();
        let name = unsafe { gl.create_texture() }?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(name));
            gl.tex_storage_2d(glow::TEXTURE_2D, 1, format as u32, width, height);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        Ok(Texture {
            gl: self.gl.clone(),
            name,
            width,
            height,
            format,
        })
    }

    /// A sampler with these filters and wrap modes.
    pub fn create_sampler(
        &self,
        min: Filter,
        mag: Filter,
        wrap_s: Wrap,
        wrap_t: Wrap,
    ) -> Result<Sampler, String> {
        let gl = self.gl.gl();
        let name = unsafe { gl.create_sampler() }?;
        unsafe {
            gl.sampler_parameter_i32(name, glow::TEXTURE_MIN_FILTER, min as i32);
            gl.sampler_parameter_i32(name, glow::TEXTURE_MAG_FILTER, mag as i32);
            gl.sampler_parameter_i32(name, glow::TEXTURE_WRAP_S, wrap_s as i32);
            gl.sampler_parameter_i32(name, glow::TEXTURE_WRAP_T, wrap_t as i32);
        }
        Ok(Sampler {
            gl: self.gl.clone(),
            name,
        })
    }

    /// A program linked from GLSL ES 3.00 vertex and fragment sources. Compile and link
    /// errors are thrown with OpenGL ES's log.
    pub fn create_program(&self, vertex: &str, fragment: &str) -> Result<Program, String> {
        let gl = self.gl.gl();
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, source, stage) in [
                (glow::VERTEX_SHADER, vertex, "vertex"),
                (glow::FRAGMENT_SHADER, fragment, "fragment"),
            ] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let log = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    for s in shaders {
                        gl.delete_shader(s);
                    }
                    gl.delete_program(program);
                    return Err(format!("compiling the {stage} shader: {log}"));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            for s in shaders {
                gl.detach_shader(program, s);
                gl.delete_shader(s);
            }
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("linking the program: {log}"));
            }
            Ok(Program {
                gl: self.gl.clone(),
                name: program,
            })
        }
    }

    /// An empty vertex array.
    pub fn create_vertex_array(&self) -> Result<VertexArray, String> {
        let gl = self.gl.gl();
        let name = unsafe { gl.create_vertex_array() }?;
        Ok(VertexArray {
            gl: self.gl.clone(),
            name,
        })
    }

    /// A framebuffer rendering into `color`. Throws unless it is complete.
    pub fn create_framebuffer(&self, color: &Texture) -> Result<Framebuffer, String> {
        let gl = self.gl.gl();
        let name = unsafe { gl.create_framebuffer() }?;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(name));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                color.format.attachment(),
                glow::TEXTURE_2D,
                Some(color.name),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(name);
                return Err(format!(
                    "the framebuffer is incomplete: status 0x{status:X}"
                ));
            }
        }
        Ok(Framebuffer {
            gl: self.gl.clone(),
            name,
        })
    }

    /// Renders into `framebuffer` from now on.
    pub fn bind_framebuffer(&self, framebuffer: &Framebuffer) {
        unsafe {
            self.gl
                .gl()
                .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer.name))
        };
    }

    /// Renders into the window from now on.
    pub fn bind_default_framebuffer(&self) {
        unsafe { self.gl.gl().bind_framebuffer(glow::FRAMEBUFFER, None) };
    }

    pub fn viewport(&self, x: i32, y: i32, width: i32, height: i32) {
        unsafe { self.gl.gl().viewport(x, y, width, height) };
    }

    pub fn scissor(&self, x: i32, y: i32, width: i32, height: i32) {
        unsafe { self.gl.gl().scissor(x, y, width, height) };
    }

    /// Clears the bound framebuffer's color, depth or stencil: `bits` are `ClearBits` or-ed
    /// together.
    pub fn clear(&self, bits: i32, r: f64, g: f64, b: f64, a: f64) {
        let gl = self.gl.gl();
        unsafe {
            gl.clear_color(r as f32, g as f32, b as f32, a as f32);
            gl.clear(bits as u32);
        }
    }

    pub fn use_program(&self, program: &Program) {
        unsafe { self.gl.gl().use_program(Some(program.name)) };
    }

    pub fn bind_vertex_array(&self, vertex_array: &VertexArray) {
        unsafe { self.gl.gl().bind_vertex_array(Some(vertex_array.name)) };
    }

    /// Binds `buffer` to uniform buffer binding point `index`.
    pub fn bind_uniform_buffer(&self, index: i32, buffer: &Buffer) -> Result<(), String> {
        let index = u32_of(index, "a binding point")?;
        unsafe {
            self.gl
                .gl()
                .bind_buffer_base(glow::UNIFORM_BUFFER, index, Some(buffer.name))
        };
        Ok(())
    }

    /// Binds `texture` and `sampler` to texture unit `unit`.
    pub fn bind_texture(
        &self,
        unit: i32,
        texture: &Texture,
        sampler: &Sampler,
    ) -> Result<(), String> {
        let unit = u32_of(unit, "a texture unit")?;
        let gl = self.gl.gl();
        unsafe {
            gl.active_texture(glow::TEXTURE0 + unit);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture.name));
            gl.bind_sampler(unit, Some(sampler.name));
        }
        Ok(())
    }

    /// `glDrawArraysInstanced`.
    pub fn draw_arrays(&self, primitive: Primitive, first: i32, count: i32, instances: i32) {
        unsafe {
            self.gl
                .gl()
                .draw_arrays_instanced(primitive as u32, first, count, instances)
        };
    }

    /// `glDrawElementsInstanced`, with indices from the bound vertex array's element buffer,
    /// starting `offset` bytes in.
    pub fn draw_elements(
        &self,
        primitive: Primitive,
        count: i32,
        kind: ComponentType,
        offset: i32,
        instances: i32,
    ) {
        unsafe {
            self.gl.gl().draw_elements_instanced(
                primitive as u32,
                count,
                kind as u32,
                offset,
                instances,
            )
        };
    }

    /// The bound framebuffer's pixels in a rectangle, as 8-bit RGBA, bottom row first.
    pub fn read_pixels(&self, x: i32, y: i32, width: i32, height: i32) -> Result<Vec<u8>, String> {
        let gl = self.gl.gl();
        let length = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .map(|(w, h)| w * h * 4)
            .ok_or("a rectangle's size cannot be negative")?;
        let mut pixels = vec![0u8; length];
        unsafe {
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                x,
                y,
                width,
                height,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        Ok(pixels)
    }

    /// `glFlush`.
    pub fn flush(&self) {
        unsafe { self.gl.gl().flush() };
    }

    /// `glFinish`: returns when the GPU has finished every command so far.
    pub fn finish(&self) {
        unsafe { self.gl.gl().finish() };
    }

    /// Settles when the GPU has finished every command so far. It waits on an EGL fence from
    /// another thread; without EGL 1.5 and `EGL_KHR_fence_sync` it finishes here and is settled
    /// already.
    pub fn completed(&self) -> Result<Future<()>, String> {
        let display = self.gl.display.clone();
        let gl = self.gl.gl();
        let fence = display
            .egl
            .0
            .upcast::<egl::EGL1_5>()
            .filter(|_| display.fences);
        let Some(egl15) = fence else {
            unsafe { gl.finish() };
            let (completer, future) = hxn_future::pending();
            completer.resolve(());
            return Ok(future);
        };
        let sync = unsafe {
            egl15.create_sync(
                display.display.0,
                egl::SYNC_FENCE as egl::Enum,
                &[egl::ATTRIB_NONE],
            )
        }
        .map_err(|e| egl_failed("creating an EGL fence", e))?;
        // The waiting thread has no context to flush, so the fence is flushed here.
        unsafe { gl.flush() };
        let sync = Shared(sync);
        Ok(hxn_future::spawn_blocking(move || {
            let sync = sync;
            let egl15 = display
                .egl
                .0
                .upcast::<egl::EGL1_5>()
                .ok_or("EGL 1.5 went away")?;
            let waited =
                unsafe { egl15.client_wait_sync(display.display.0, sync.0, 0, egl::FOREVER) }
                    .map_err(|e| egl_failed("waiting on the EGL fence", e));
            let _ = unsafe { egl15.destroy_sync(display.display.0, sync.0) };
            waited.map(|_| ())
        }))
    }
}

/// A buffer object.
#[hxn::interface]
pub struct Buffer {
    gl: Arc<Gl>,
    name: glow::Buffer,
    target: u32,
    size: i32,
}

#[hxn::methods]
impl Buffer {
    pub fn size(&self) -> i64 {
        self.size as i64
    }

    /// Writes `bytes` at `offset`.
    pub fn write(&self, offset: i64, bytes: &[u8]) -> Result<(), String> {
        let offset = i32_of(offset, "an offset")?;
        if offset < 0 || offset as i64 + bytes.len() as i64 > self.size as i64 {
            return Err(format!(
                "{} bytes at {offset} overrun a buffer of {}",
                bytes.len(),
                self.size
            ));
        }
        let gl = self.gl.gl();
        unsafe {
            gl.bind_buffer(self.target, Some(self.name));
            gl.buffer_sub_data_u8_slice(self.target, offset, bytes);
            gl.bind_buffer(self.target, None);
        }
        Ok(())
    }

    /// Reads `length` bytes at `offset`, mapping the buffer. It waits for the GPU to finish
    /// writing them.
    pub fn read(&self, offset: i64, length: i64) -> Result<Vec<u8>, String> {
        let offset = i32_of(offset, "an offset")?;
        let length = i32_of(length, "a length")?;
        if offset < 0 || length < 0 || offset as i64 + length as i64 > self.size as i64 {
            return Err(format!(
                "{length} bytes at {offset} overrun a buffer of {}",
                self.size
            ));
        }
        let gl = self.gl.gl();
        unsafe {
            gl.bind_buffer(glow::COPY_READ_BUFFER, Some(self.name));
            let mapped =
                gl.map_buffer_range(glow::COPY_READ_BUFFER, offset, length, glow::MAP_READ_BIT);
            if mapped.is_null() {
                gl.bind_buffer(glow::COPY_READ_BUFFER, None);
                return Err("mapping the buffer failed".into());
            }
            let bytes = std::slice::from_raw_parts(mapped, length as usize).to_vec();
            gl.unmap_buffer(glow::COPY_READ_BUFFER);
            gl.bind_buffer(glow::COPY_READ_BUFFER, None);
            Ok(bytes)
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.gl.release(Name::Buffer(self.name));
    }
}

/// A 2D texture of one level.
#[hxn::interface]
pub struct Texture {
    gl: Arc<Gl>,
    name: glow::Texture,
    width: i32,
    height: i32,
    format: TextureFormat,
}

#[hxn::methods]
impl Texture {
    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// Writes a rectangle of pixels, rows tightly packed, bottom row first.
    pub fn write(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        pixels: &[u8],
    ) -> Result<(), String> {
        let expected = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .map(|(w, h)| w * h * self.format.bytes_per_pixel())
            .ok_or("a rectangle's size cannot be negative")?;
        if pixels.len() != expected {
            return Err(format!(
                "{width}x{height} pixels are {expected} bytes, not {}",
                pixels.len()
            ));
        }
        let (format, kind) = self.format.transfer();
        let gl = self.gl.gl();
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.name));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x,
                y,
                width,
                height,
                format,
                kind,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        Ok(())
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        self.gl.release(Name::Texture(self.name));
    }
}

/// A sampler object.
#[hxn::interface]
pub struct Sampler {
    gl: Arc<Gl>,
    name: glow::Sampler,
}

#[hxn::methods]
impl Sampler {}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.gl.release(Name::Sampler(self.name));
    }
}

/// A linked program.
#[hxn::interface]
pub struct Program {
    gl: Arc<Gl>,
    name: glow::Program,
}

#[hxn::methods]
impl Program {
    /// Assigns the uniform block `name` to binding point `binding`. False when the program
    /// has no such block.
    pub fn uniform_block(&self, name: &str, binding: i32) -> Result<bool, String> {
        let binding = u32_of(binding, "a binding point")?;
        let gl = self.gl.gl();
        unsafe {
            match gl.get_uniform_block_index(self.name, name) {
                Some(index) => {
                    gl.uniform_block_binding(self.name, index, binding);
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }

    /// Assigns the sampler uniform `name` to texture unit `unit`. False when the program has
    /// no such uniform. It leaves this program in use.
    pub fn sampler(&self, name: &str, unit: i32) -> bool {
        let gl = self.gl.gl();
        unsafe {
            match gl.get_uniform_location(self.name, name) {
                Some(location) => {
                    gl.use_program(Some(self.name));
                    gl.uniform_1_i32(Some(&location), unit);
                    true
                }
                None => false,
            }
        }
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        self.gl.release(Name::Program(self.name));
    }
}

/// A vertex array: attribute layouts and the buffers they read, and an element buffer.
#[hxn::interface]
pub struct VertexArray {
    gl: Arc<Gl>,
    name: glow::VertexArray,
}

#[hxn::methods]
impl VertexArray {
    /// Attribute `location` reads `size` components of `kind` from `buffer`, `stride` bytes
    /// apart from `offset`, as floats; integer components become floats, scaled to 0-1 or
    /// -1-1 when `normalized`.
    #[allow(clippy::too_many_arguments)]
    pub fn attribute(
        &self,
        location: i32,
        buffer: &Buffer,
        size: i32,
        kind: ComponentType,
        normalized: bool,
        stride: i32,
        offset: i32,
    ) -> Result<(), String> {
        let location = u32_of(location, "a location")?;
        let gl = self.gl.gl();
        unsafe {
            gl.bind_vertex_array(Some(self.name));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer.name));
            gl.vertex_attrib_pointer_f32(location, size, kind as u32, normalized, stride, offset);
            gl.enable_vertex_attrib_array(location);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
        }
        Ok(())
    }

    /// Attribute `location` reads `size` integer components of `kind`, for an `int` or
    /// `uint` input.
    pub fn integer_attribute(
        &self,
        location: i32,
        buffer: &Buffer,
        size: i32,
        kind: ComponentType,
        stride: i32,
        offset: i32,
    ) -> Result<(), String> {
        let location = u32_of(location, "a location")?;
        let gl = self.gl.gl();
        unsafe {
            gl.bind_vertex_array(Some(self.name));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer.name));
            gl.vertex_attrib_pointer_i32(location, size, kind as u32, stride, offset);
            gl.enable_vertex_attrib_array(location);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
        }
        Ok(())
    }

    /// Attribute `location` advances once per `divisor` instances; 0 advances per vertex.
    pub fn divisor(&self, location: i32, divisor: i32) -> Result<(), String> {
        let location = u32_of(location, "a location")?;
        let divisor = u32_of(divisor, "a divisor")?;
        let gl = self.gl.gl();
        unsafe {
            gl.bind_vertex_array(Some(self.name));
            gl.vertex_attrib_divisor(location, divisor);
            gl.bind_vertex_array(None);
        }
        Ok(())
    }

    /// The buffer `drawElements` reads indices from.
    pub fn element_buffer(&self, buffer: &Buffer) {
        let gl = self.gl.gl();
        unsafe {
            gl.bind_vertex_array(Some(self.name));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffer.name));
            gl.bind_vertex_array(None);
        }
    }
}

impl Drop for VertexArray {
    fn drop(&mut self) {
        self.gl.release(Name::VertexArray(self.name));
    }
}

/// A framebuffer with one attachment.
#[hxn::interface]
pub struct Framebuffer {
    gl: Arc<Gl>,
    name: glow::Framebuffer,
}

#[hxn::methods]
impl Framebuffer {}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        self.gl.release(Name::Framebuffer(self.name));
    }
}
