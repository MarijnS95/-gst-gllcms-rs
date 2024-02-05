use std::{convert::TryInto, sync::Mutex};

use gst_gl::{
    gst::{glib, subclass::ElementMetadata},
    gst_base::subclass::{prelude::*, BaseTransformMode},
    prelude::*,
    subclass::{prelude::*, GLFilterMode},
    *,
};
use lcms2::*;
use once_cell::sync::Lazy;

// Default vertex shader from gst_gl_shader_string_vertex_default
const VERTEX_SHADER: &str = r"
in vec4 a_position;
in vec2 a_texcoord;
out vec2 v_texcoord;
void main()
{
   gl_Position = a_position;
   v_texcoord = a_texcoord;
}";

const FRAGMENT_SHADER: &str = r"
in vec2 v_texcoord;
out vec4 fragColor;

uniform sampler2D tex;
layout(binding = 0)
buffer lutTable
{
    int lut[];
};

void main () {
    vec4 rgba = texture(tex, v_texcoord);
    if (v_texcoord.y > 0.5) {
        fragColor = rgba;
    } else {
        vec4 rgb_ = vec4(rgba.xyz, 0);
        uint idx = packUnorm4x8(rgb_);
        vec3 rgb = unpackUnorm4x8(lut[idx]).xyz;
        fragColor = vec4(rgb, 1);
    }
}
";

const DEFAULT_BRIGHTNESS: f64 = 0f64;
const DEFAULT_CONTRAST: f64 = 1f64;
const DEFAULT_HUE: f64 = 0f64;
const DEFAULT_SATURATION: f64 = 0f64;

#[derive(Debug, Clone, PartialEq)]
struct Settings {
    icc: Option<String>,
    brightness: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            icc: None,
            brightness: DEFAULT_BRIGHTNESS,
            contrast: DEFAULT_CONTRAST,
            hue: DEFAULT_HUE,
            saturation: DEFAULT_SATURATION,
        }
    }
}

struct State {
    shader: GLShader,
    gl: gl::Gl,
    lut_buffer: gl::types::GLuint,
    current_settings: Option<Settings>,
}

#[derive(Default)]
pub struct GlLcms {
    // TODO: Need multi-reader lock?
    settings: Mutex<Settings>,
    state: Mutex<Option<State>>,
}

static PROPERTIES: Lazy<[glib::ParamSpec; 5]> = Lazy::new(|| {
    [
        glib::ParamSpecString::builder("icc")
            .nick("ICC Profile")
            .blurb("Path to ICC color profile")
            .build(),
        glib::ParamSpecDouble::builder("brightness")
            .nick("Bright")
            .blurb("Extra brightness correction")
            // TODO: Docs don't clarify min and max!
            .minimum(f64::MIN)
            .maximum(f64::MAX)
            .default_value(DEFAULT_BRIGHTNESS)
            .build(),
        glib::ParamSpecDouble::builder("contrast")
            .nick("Contrast")
            .blurb("Extra contrast correction")
            // TODO: Docs don't clarify min and max!
            .minimum(f64::MIN)
            .maximum(f64::MAX)
            .default_value(DEFAULT_CONTRAST)
            .build(),
        glib::ParamSpecDouble::builder("hue")
            .nick("Hue")
            .blurb("Extra hue displacement in degrees")
            .minimum(0f64)
            .maximum(360f64)
            .default_value(DEFAULT_HUE)
            .build(),
        glib::ParamSpecDouble::builder("saturation")
            .nick("Saturation")
            .blurb("Extra saturation correction")
            // TODO: Docs don't clarify min and max!
            .minimum(f64::MIN)
            .maximum(f64::MAX)
            .default_value(DEFAULT_SATURATION)
            .build(),
        // TODO: Model white balance src+dest as structure
        // glib::ParamSpec::new_value_array(
        //     "temp",
        //     "Source temperature",
        //     "Source white point temperature",
        //     &glib::ParamSpec::new_uint(
        //         "the temperature",
        //         "Source temperature",
        //         "Source white point temperature",
        //         // TODO: Docs don't clarify min and max!
        //         0,
        //         std::u32::MAX,
        //         0,
        //         glib::ParamFlags::READWRITE,
        //     ),
        //     glib::ParamFlags::READWRITE,
        // ),
    ]
});

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "gllcms",
        gst::DebugColorFlags::empty(),
        Some("Rust LCMS2-based color correction in OpenGL"),
    )
});

#[glib::object_subclass]
impl ObjectSubclass for GlLcms {
    const NAME: &'static str = "gllcms";
    type ParentType = GLFilter;
    type Type = super::GlLcms;
}

impl ObjectImpl for GlLcms {
    fn properties() -> &'static [glib::ParamSpec] {
        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        // assert_eq!(pspec, PROPERTIES[id]);

        gst::info!(CAT, imp: self, "Changing {:?} to {:?}", pspec, value);

        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "icc" => settings.icc = value.get().expect("Type mismatch"),
            "brightness" => settings.brightness = value.get().expect("Type mismatch"),
            "contrast" => settings.contrast = value.get().expect("Type mismatch"),
            "hue" => settings.hue = value.get().expect("Type mismatch"),
            "saturation" => settings.saturation = value.get().expect("Type mismatch"),
            _ => {
                // This means someone added a property to PROPERTIES but forgot to handle it here...
                gst::error!(CAT, imp: self, "Can't handle {:?}", pspec);
                panic!("set_property unhandled for {:?}", pspec);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        let settings = self.settings.lock().unwrap();

        match pspec.name() {
            "icc" => settings.icc.to_value(),
            "brightness" => settings.brightness.to_value(),
            "contrast" => settings.contrast.to_value(),
            "hue" => settings.hue.to_value(),
            "saturation" => settings.saturation.to_value(),
            _ => {
                gst::error!(CAT, imp: self, "Can't handle {:?}", pspec);
                panic!("get_property unhandled for {:?}", pspec);
            }
        }
    }
}

impl GstObjectImpl for GlLcms {}

impl ElementImpl for GlLcms {
    fn metadata() -> Option<&'static ElementMetadata> {
        static ELEMENT_METADATA: Lazy<ElementMetadata> = Lazy::new(|| {
            ElementMetadata::new(
                "Rust LCMS2-based color correction in OpenGL",
                "Filter/Effect/Converter/Video",
                env!("CARGO_PKG_DESCRIPTION"),
                env!("CARGO_PKG_AUTHORS"),
            )
        });

        Some(&*ELEMENT_METADATA)
    }
}

impl BaseTransformImpl for GlLcms {
    const MODE: BaseTransformMode = BaseTransformMode::NeverInPlace;
    const PASSTHROUGH_ON_SAME_CAPS: bool = false;
    const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
}

fn create_shader(imp: &GlLcms, context: &GLContext) -> GLShader {
    let shader = GLShader::new(context);
    // 400 For (un)packUnorm
    // 430 for SSBO (https://www.khronos.org/opengl/wiki/Shader_Storage_Buffer_Object)
    let version = GLSLVersion::_430;
    let profile = GLSLProfile::empty();
    let shader_version = format!(
        "#version {}",
        &GLSLVersion::profile_to_string(version, profile).unwrap()
    );

    // let vertex = GLSLStage::new_default_vertex(context);
    // new_default_vertex assumes GLSLVersion::None and ES | COMPATIBILITY profile
    let shader_parts = [&shader_version, VERTEX_SHADER];

    gst::debug!(
        CAT,
        imp: imp,
        "Compiling vertex shader parts {:?}",
        &shader_parts
    );

    let vertex =
        GLSLStage::with_strings(context, gl::VERTEX_SHADER, version, profile, &shader_parts);
    vertex.compile().unwrap();
    shader.attach_unlocked(&vertex).unwrap();

    let shader_parts = [&shader_version, FRAGMENT_SHADER];

    gst::debug!(
        CAT,
        imp: imp,
        "Compiling fragment shader parts {:?}",
        &shader_parts
    );

    let fragment = GLSLStage::with_strings(
        context,
        gl::FRAGMENT_SHADER,
        version,
        profile,
        &shader_parts,
    );
    fragment.compile().unwrap();
    shader.attach_unlocked(&fragment).unwrap();
    shader.link().unwrap();

    gst::debug!(CAT, imp: imp, "Successfully linked {:?}", shader);

    shader
}

fn create_ssbo(gl: &gl::Gl) -> u32 {
    let mut ssbo = std::mem::MaybeUninit::uninit();
    unsafe {
        gl.GenBuffers(1, ssbo.as_mut_ptr());
        ssbo.assume_init()
    }
}

impl GLBaseFilterImpl for GlLcms {
    fn gl_start(&self) -> Result<(), gst::LoggableError> {
        gst::debug!(CAT, imp: self, "gl_start");

        let obj = self.obj();
        let context = obj.context().unwrap();
        let mut state = self.state.lock().unwrap();

        let shader = create_shader(self, &context);

        // TODO: Should perhaps use Gst types, even though they appear to implement more complex and unnecessary features like automatic CPU mapping/copying
        let gl = gl::Gl::load_with(|fn_name| context.proc_address(fn_name) as _);

        let lut_buffer = create_ssbo(&gl);

        gst::trace!(
            CAT,
            imp: self,
            "Created SSBO containing lut at {lut_buffer:?}"
        );

        let was_uninitialized = state
            .replace(State {
                shader,
                gl,
                lut_buffer,
                current_settings: None,
            })
            .is_none();
        assert!(
            was_uninitialized,
            "State mut not have already been initialized when calling gl_stop()"
        );

        self.parent_gl_start()
    }

    fn gl_stop(&self) {
        gst::debug!(CAT, imp: self, "gl_stop");

        let mut state = self.state.lock().unwrap();
        let _ = state
            .take()
            .expect("State must have been initialized when calling gl_stop()");

        self.parent_gl_stop()
    }
}

impl GLFilterImpl for GlLcms {
    const MODE: GLFilterMode = GLFilterMode::Texture;

    fn filter_texture(
        &self,
        input: &GLMemory,
        output: &GLMemory,
    ) -> Result<(), gst::LoggableError> {
        let obj = self.obj();
        let mut state = self.state.lock().unwrap();
        let state = state
            .as_mut()
            .expect("Should not be calling filter_texture() before gl_start() or after gl_stop()");

        // Unpack references to struct members
        let State {
            shader,
            gl,
            lut_buffer,
            current_settings,
        } = state;
        let lut_buffer = *lut_buffer;

        let settings = &*self.settings.lock().unwrap();
        if current_settings.as_ref() != Some(settings) {
            gst::trace!(CAT, imp: self, "Settings changed, updating LUT");

            if settings == &Default::default() {
                gst::warning!(
                    CAT,
                    imp: self,
                    "gllcms without options does nothing, performing mem -> mem copy"
                );

                // unsafe { input.memcpy(&mut output, output.offset(), output.size()) };

                todo!("Implement memcpy");
                // return true;
            }

            gst::info!(CAT, imp: self, "Creating LUT from {:?}", settings);

            let mut profiles = vec![];

            if let Some(icc) = &settings.icc {
                let custom_profile = Profile::new_file(icc).unwrap();
                profiles.push(custom_profile);
            }

            // TODO: Put these four settings in a separate struct for easy Default comparison and elision
            let bcsh = Profile::new_bchsw_abstract_context(
                GlobalContext::new(),
                // Can't have more than 255 points... Is this per-axis (as it's rather slow)?
                255,
                settings.brightness,
                settings.contrast,
                settings.hue,
                settings.saturation,
                /* No color temperature support yet */ None,
            )
            .unwrap();
            profiles.push(bcsh);

            // Use sRGB as output profile, last in the chain
            let output_profile = Profile::new_srgb();

            // TODO: bcsh on its own breaks Transform construction

            let t = if let [single_profile] = &profiles[..] {
                Transform::new(
                    single_profile,
                    PixelFormat::RGBA_8,
                    &output_profile,
                    PixelFormat::RGBA_8,
                    Intent::Perceptual,
                )
                .unwrap()
            } else {
                // Output profile is last in the chain
                profiles.push(output_profile);

                // Turn into vec of references
                let profiles = profiles.iter().collect::<Vec<_>>();
                Transform::new_multiprofile(
                    &profiles,
                    PixelFormat::RGBA_8,
                    PixelFormat::RGBA_8,
                    Intent::Perceptual,
                    // TODO: Check all flags
                    Flags::NO_NEGATIVES | Flags::KEEP_SEQUENCE,
                )
                .unwrap()
            };

            let mut source_pixels = (0..0x1_00_00_00).collect::<Vec<_>>();
            t.transform_in_place(&mut source_pixels);

            // Bind in SSBO slot and upload data
            unsafe { gl.BindBuffer(gl::SHADER_STORAGE_BUFFER, lut_buffer) };
            unsafe {
                // BufferStorage to keep the buffer mutable, in contrast to BufferStorage
                gl.BufferStorage(
                    gl::SHADER_STORAGE_BUFFER,
                    (source_pixels.len() * std::mem::size_of::<u32>())
                        .try_into()
                        .unwrap(),
                    source_pixels.as_ptr().cast(),
                    0,
                )
            };

            state.current_settings = Some(settings.clone());
        }

        // Bind the shader in advance to be able to bind our storage buffer
        shader.use_();

        // Actually bind the lut to `uint lut[];`
        unsafe { gl.BindBuffer(gl::SHADER_STORAGE_BUFFER, lut_buffer) };
        unsafe {
            gl.BindBufferBase(
                gl::SHADER_STORAGE_BUFFER,
                /* binding 0 */ 0,
                lut_buffer,
            )
        };

        obj.render_to_target_with_shader(input, output, shader);

        // Cleanup
        unsafe { gl.BindBuffer(gl::SHADER_STORAGE_BUFFER, 0) };

        gst::trace!(CAT, imp: self, "Render finished");

        self.parent_filter_texture(input, output)
    }
}
