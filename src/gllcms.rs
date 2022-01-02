use gst::glib;
use gst::subclass::ElementMetadata;
use gst_base::subclass::BaseTransformMode;
use gst_gl::gst_base::subclass::prelude::*;
use gst_gl::prelude::*;
use gst_gl::subclass::prelude::*;
use gst_gl::subclass::GLFilterMode;
use gst_gl::*;

use lcms2::*;
use once_cell::sync::Lazy;
use std::convert::TryInto;
use std::sync::Mutex;

// Default vertex shader from gst_gl_shader_string_vertex_default
const VERTEX_SHADER: &str = r#"
in vec4 a_position;
in vec2 a_texcoord;
out vec2 v_texcoord;
void main()
{
   gl_Position = a_position;
   v_texcoord = a_texcoord;
}"#;

const FRAGMENT_SHADER: &str = r#"
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
"#;

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
        glib::ParamSpecString::new(
            "icc",
            "ICC Profile",
            "Path to ICC color profile",
            None,
            glib::ParamFlags::READWRITE,
        ),
        glib::ParamSpecDouble::new(
            "brightness",
            "Bright",
            "Extra brightness correction",
            // TODO: Docs don't clarify min and max!
            f64::MIN,
            f64::MAX,
            DEFAULT_BRIGHTNESS,
            glib::ParamFlags::READWRITE,
        ),
        glib::ParamSpecDouble::new(
            "contrast",
            "Contrast",
            "Extra contrast correction",
            // TODO: Docs don't clarify min and max!
            f64::MIN,
            f64::MAX,
            DEFAULT_CONTRAST,
            glib::ParamFlags::READWRITE,
        ),
        glib::ParamSpecDouble::new(
            "hue",
            "Hue",
            "Extra hue displacement in degrees",
            0f64,
            360f64,
            DEFAULT_HUE,
            glib::ParamFlags::READWRITE,
        ),
        glib::ParamSpecDouble::new(
            "saturation",
            "Saturation",
            "Extra saturation correction",
            // TODO: Docs don't clarify min and max!
            f64::MIN,
            f64::MAX,
            DEFAULT_SATURATION,
            glib::ParamFlags::READWRITE,
        ),
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

    fn set_property(
        &self,
        element: &Self::Type,
        _id: usize,
        value: &glib::Value,
        pspec: &glib::ParamSpec,
    ) {
        // assert_eq!(pspec, PROPERTIES[id]);

        gst::gst_info!(CAT, obj: element, "Changing {:?} to {:?}", pspec, value);

        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "icc" => settings.icc = value.get().expect("Type mismatch"),
            "brightness" => settings.brightness = value.get().expect("Type mismatch"),
            "contrast" => settings.contrast = value.get().expect("Type mismatch"),
            "hue" => settings.hue = value.get().expect("Type mismatch"),
            "saturation" => settings.saturation = value.get().expect("Type mismatch"),
            _ => {
                // This means someone added a property to PROPERTIES but forgot to handle it here...
                gst::gst_error!(CAT, obj: element, "Can't handle {:?}", pspec);
                panic!("set_property unhandled for {:?}", pspec);
            }
        }
    }

    fn property(&self, element: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        let settings = self.settings.lock().unwrap();

        match pspec.name() {
            "icc" => settings.icc.to_value(),
            "brightness" => settings.brightness.to_value(),
            "contrast" => settings.contrast.to_value(),
            "hue" => settings.hue.to_value(),
            "saturation" => settings.saturation.to_value(),
            _ => {
                gst::gst_error!(CAT, obj: element, "Can't handle {:?}", pspec);
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
impl GLBaseFilterImpl for GlLcms {}

fn create_shader(filter: &super::GlLcms, context: &GLContext) -> GLShader {
    let shader = GLShader::new(context);
    // 400 For (un)packUnorm
    // 430 for SSBO (https://www.khronos.org/opengl/wiki/Shader_Storage_Buffer_Object)
    let version = GLSLVersion::_430;
    let profile = GLSLProfile::empty();

    // let vertex = GLSLStage::new_default_vertex(context);
    // new_default_vertex assumes GLSLVersion::None and ES | COMPATIBILITY profile
    let shader_parts = [
        &format!(
            "#version {}",
            &GLSLVersion::profile_to_string(version, profile).unwrap()
        ),
        VERTEX_SHADER,
    ];

    gst::gst_debug!(
        CAT,
        obj: filter,
        "Compiling vertex shader parts {:?}",
        &shader_parts
    );

    let vertex =
        GLSLStage::with_strings(context, gl::VERTEX_SHADER, version, profile, &shader_parts);
    vertex.compile().unwrap();
    shader.attach_unlocked(&vertex).unwrap();

    let shader_parts = [
        &format!(
            "#version {}",
            &GLSLVersion::profile_to_string(version, profile).unwrap()
        ),
        FRAGMENT_SHADER,
    ];

    gst::gst_debug!(
        CAT,
        obj: filter,
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

    gst::gst_debug!(CAT, obj: filter, "Successfully linked {:?}", shader);

    shader
}

fn create_ssbo(gl: &gl::Gl) -> u32 {
    let mut ssbo = std::mem::MaybeUninit::uninit();
    unsafe {
        gl.GenBuffers(1, ssbo.as_mut_ptr());
        ssbo.assume_init()
    }
}

impl GLFilterImpl for GlLcms {
    const MODE: GLFilterMode = GLFilterMode::Texture;

    fn filter_texture(
        &self,
        filter: &Self::Type,
        input: &GLMemory,
        output: &GLMemory,
    ) -> Result<(), gst::LoggableError> {
        let context = filter.context().unwrap();

        let mut state = self.state.lock().unwrap();

        let state = if let Some(state) = &mut *state {
            state
        } else {
            let shader = create_shader(filter, &context);

            // TODO: Should perhaps use Gst types, even though they appear to implement more complex complex and unnecessary features like automatic CPU mapping/copying
            let gl = gl::Gl::load_with(|fn_name| context.proc_address(fn_name) as _);

            let lut_buffer = create_ssbo(&gl);
            gst::gst_trace!(
                CAT,
                obj: filter,
                "Created SSBO containing lut at {:?}",
                lut_buffer
            );

            *state = Some(State {
                shader,
                gl,
                lut_buffer,
                current_settings: None,
            });
            state.as_mut().unwrap()
        };

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
            gst::gst_trace!(CAT, obj: filter, "Settings changed, updating LUT");

            if settings == &Default::default() {
                gst::gst_warning!(
                    CAT,
                    obj: filter,
                    "gllcms without options does nothing, performing mem -> mem copy"
                );

                todo!("Implement memcpy");
                // return true;
            }

            gst::gst_info!(CAT, obj: filter, "Creating LUT from {:?}", settings);

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

        filter.render_to_target_with_shader(input, output, shader);

        // Cleanup
        unsafe { gl.BindBuffer(gl::SHADER_STORAGE_BUFFER, 0) };

        gst::gst_trace!(CAT, obj: filter, "Render finished");

        self.parent_filter_texture(filter, input, output)
    }
}
