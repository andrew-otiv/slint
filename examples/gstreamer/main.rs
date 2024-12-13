// Copyright © SixtyFPS GmbH <info@slint-ui.com>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-commercial

use std::sync::{Arc, Mutex};

use gstreamer::prelude::*;
use gstreamer_gl::prelude::*;
use gstreamer_gl::GLVideoFrameExt;
use gstreamer_video::VideoFrameExt;

slint::include_modules!();

struct GstGlContext {
    context: gstreamer_gl::GLContext,
    display: gstreamer_gl_egl::GLDisplayEGL,
}

// #[derive(Clone)]
struct PerWindowData<C: slint::ComponentHandle + 'static> {
    window: slint::Weak<C>,
    appsink: gstreamer_app::AppSink,
    current_sample: std::sync::Arc<std::sync::Mutex<Option<gstreamer::Sample>>>,
    gst_gl_context: Arc<Mutex<Option<GstGlContext>>>,
}

struct Player<C: slint::ComponentHandle + 'static> {
    per_window_data1: PerWindowData<C>,
    per_window_data2: PerWindowData<C>,
    pipeline: gstreamer::Pipeline,
}

impl<C: slint::ComponentHandle + 'static> Player<C> {
    fn new(window1: slint::Weak<C>, window2: slint::Weak<C>) -> Result<Self, anyhow::Error> {
        gstreamer::init()?;
        let pipeline = gstreamer::Pipeline::with_name("pipeline");

        let source = gstreamer::ElementFactory::make("playbin").build()?;
        source.set_property("uri", "https://www.freedesktop.org/software/gstreamer-sdk/data/media/sintel_trailer-480p.webm");

        let tee = gstreamer::ElementFactory::make("tee").build()?;
        let tee_pad1 = tee.request_pad_simple("src_%u").unwrap();
        let tee_pad2 = tee.request_pad_simple("src_%u").unwrap();

        let appsink1 = gstreamer::ElementFactory::make("appsink")
            .build()?
            .dynamic_cast::<gstreamer_app::AppSink>()
            .unwrap();

        let appsink2 = gstreamer::ElementFactory::make("appsink")
            .build()?
            .dynamic_cast::<gstreamer_app::AppSink>()
            .unwrap();

        appsink1.set_property("enable-last-sample", false);
        appsink1.set_property("emit-signals", false);
        appsink1.set_property("max-buffers", 1u32);

        appsink2.set_property("enable-last-sample", false);
        appsink2.set_property("emit-signals", false);
        appsink2.set_property("max-buffers", 1u32);

        let caps = gstreamer::Caps::builder("video/x-raw")
            .features([gstreamer_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
            .field("format", gstreamer_video::VideoFormat::Rgba.to_str())
            .field("texture-target", "2D")
            .build();
        appsink1.set_caps(Some(&caps));
        appsink2.set_caps(Some(&caps));

        let glsink1 = gstreamer::ElementFactory::make("glsinkbin").name("glsink1").build()?;
        glsink1.set_property("sink", &appsink1);
        let glsink2 = gstreamer::ElementFactory::make("glsinkbin").name("glsink2").build()?;
        glsink2.set_property("sink", &appsink2);

        pipeline.add_many([&source, &tee, &glsink1, &glsink2])?;

        let sink_pad1 = glsink1.static_pad("sink").unwrap();
        let sink_pad2 = glsink2.static_pad("sink").unwrap();
        tee_pad1.link(&sink_pad1)?;
        tee_pad2.link(&sink_pad2)?;

        Ok(Self {
            per_window_data1: PerWindowData {
                window: window1,
                appsink: appsink1,
                current_sample: std::sync::Arc::new(std::sync::Mutex::new(None)),
                gst_gl_context: Arc::new(Mutex::new(None)),
            },
            per_window_data2: PerWindowData {
                window: window2,
                appsink: appsink2,
                current_sample: std::sync::Arc::new(std::sync::Mutex::new(None)),
                gst_gl_context: Arc::new(Mutex::new(None)),
            },
            pipeline,
        })
    }

    fn setup_bus_handler(&mut self) {
        let bus = self.pipeline.bus().unwrap();
        bus.set_sync_handler({
            let context1 = self.per_window_data1.gst_gl_context.clone();
            let context2 = self.per_window_data2.gst_gl_context.clone();

            move |_, msg| {
                match msg.view() {
                    gstreamer::MessageView::NeedContext(context_messsage) => {
                        let parent_name = msg.src().unwrap().parent().unwrap().name();
                        let context = match parent_name.as_str() {
                            "glsink1" => &context1,
                            "glsink2" => &context2,
                            _ => panic!("unexpected parent!"),
                        };
                        let ctx_type = context_messsage.context_type();
                        let gst_gl_context_guard = context.lock().unwrap();
                        let gst_gl_context = gst_gl_context_guard.as_ref().unwrap();
                        if ctx_type == *gstreamer_gl::GL_DISPLAY_CONTEXT_TYPE {
                            if let Some(element) = msg.src().map(|source| {
                                source.clone().downcast::<gstreamer::Element>().unwrap()
                            }) {
                                let gst_context = gstreamer::Context::new(ctx_type, true);
                                gst_context.set_gl_display(&gst_gl_context.display);
                                element.set_context(&gst_context);
                            }
                        } else if ctx_type == "gst.gl.app_context" {
                            if let Some(element) = msg.src().map(|source| {
                                source.clone().downcast::<gstreamer::Element>().unwrap()
                            }) {
                                let mut gst_context = gstreamer::Context::new(ctx_type, true);
                                {
                                    let gst_context = gst_context.get_mut().unwrap();
                                    let structure = gst_context.structure_mut();
                                    structure.set("context", &gst_gl_context.context);
                                }
                                element.set_context(&gst_context);
                            }
                        }
                    }
                    _ => (),
                }

                gstreamer::BusSyncReply::Pass
            }
        });
    }
}

impl<C: slint::ComponentHandle> PerWindowData<C> {
    fn setup_graphics(&self, graphics_api: &slint::GraphicsAPI) {
        let egl = match graphics_api {
            slint::GraphicsAPI::NativeOpenGL { get_proc_address } => {
                glutin_egl_sys::egl::Egl::load_with(|symbol| {
                    get_proc_address(&std::ffi::CString::new(symbol).unwrap())
                })
            }
            _ => panic!("unsupported graphics API"),
        };

        {
            let mut context = self.gst_gl_context.lock().unwrap();
            if context.is_none() {
                let (gst_gl_context, gst_gl_display) = unsafe {
                    let platform = gstreamer_gl::GLPlatform::EGL;

                    let egl_display = egl.GetCurrentDisplay();
                    let display =
                        gstreamer_gl_egl::GLDisplayEGL::with_egl_display(egl_display as usize)
                            .unwrap();
                    let native_context = egl.GetCurrentContext();
                    println!("Created GL context");

                    (
                        gstreamer_gl::GLContext::new_wrapped(
                            &display,
                            native_context as _,
                            platform,
                            gstreamer_gl::GLContext::current_gl_api(platform).0,
                        )
                        .expect("unable to create wrapped GL context"),
                        display,
                    )
                };

                gst_gl_context.activate(true).expect("could not activate GSL GL context");
                gst_gl_context.fill_info().expect("failed to fill GL info for wrapped context");

                *context = Some(GstGlContext { context: gst_gl_context, display: gst_gl_display });
            } else {
                println!("Shared GL context already created");
            }
        }
    }

    fn set_appsink_callback(&self) {
        let app_weak = self.window.clone();

        let current_sample_ref = self.current_sample.clone();

        self.appsink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().unwrap();

                    {
                        let _info = sample
                            .caps()
                            .map(|caps| gstreamer_video::VideoInfo::from_caps(caps).unwrap())
                            .unwrap();
                    }

                    let current_sample_ref = current_sample_ref.clone();

                    app_weak
                        .upgrade_in_event_loop(move |app| {
                            *current_sample_ref.lock().unwrap() = Some(sample);

                            app.window().request_redraw();
                        })
                        .ok();
                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        )
    }
}

impl<C: slint::ComponentHandle + 'static> Drop for Player<C> {
    fn drop(&mut self) {
        self.pipeline.send_event(gstreamer::event::Eos::new());
        self.pipeline.set_state(gstreamer::State::Null).unwrap();
        eprintln!("Player drop");
    }
}

pub trait SetRenderingNotifier {
    fn set_rendering_notifier(per_window_data: &'static PerWindowData<VideoWindow>);
}

impl SetRenderingNotifier for PerWindowData<> {
fn set_rendering_notifier(per_window_data: &'static PerWindowData<VideoWindow>) {
    let video_window = per_window_data.window.upgrade().unwrap();
    let video_window_ = video_window.clone_strong();
    let video_window__ = video_window.clone_strong();
    let video_window_window = video_window__.window();
    video_window_window
        .set_rendering_notifier(move |state, graphics_api| match state {
            slint::RenderingState::RenderingSetup => {
                per_window_data.setup_graphics(graphics_api);
            }
            slint::RenderingState::RenderingTeardown => {
                todo!()
            }
            slint::RenderingState::BeforeRendering => {
                if let Some(sample) = per_window_data.current_sample.lock().unwrap().as_ref() {
                    let buffer = sample.buffer_owned().unwrap();
                    let info = sample
                        .caps()
                        .map(|caps| gstreamer_video::VideoInfo::from_caps(caps).unwrap())
                        .unwrap();
                    if let Ok(current_frame) =
                        gstreamer_gl::GLVideoFrame::from_buffer_readable(buffer, &info)
                    {
                        let texture =
                            current_frame.texture_id(0).expect("Failed to get gl texture id");
                        let texture = std::num::NonZero::try_from(texture)
                            .expect("Failed to get non zero texture id");
                        let size = [current_frame.width(), current_frame.height()].into();
                        let image = unsafe {
                            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                                texture, size,
                            )
                        };
                        let image = image.build();
                        video_window_.global::<MainCameraAdapter>().set_video_frame(image.clone())
                    }
                }
            }
            _ => {}
        })
        .unwrap();
}

pub fn main() -> Result<(), anyhow::Error> {
    let video_window1 = VideoWindow::new()?;
    let video_window2 = VideoWindow::new()?;
    let button_window = OtherWindow::new()?;
    let video_window1_clone = video_window1.clone_strong();
    let video_window2_clone = video_window2.clone_strong();

    let mut player = Player::new(video_window1.as_weak(), video_window2.as_weak())?;

    player.setup_bus_handler();

    let gst_gl_context: Arc<Mutex<Option<GstGlContext>>> = Arc::new(Mutex::new(None));
    player.per_window_data1.set_appsink_callback();
    player.per_window_data2.set_appsink_callback();
    set_rendering_notifier(&player.per_window_data1);
    set_rendering_notifier(&player.per_window_data2);

    player.pipeline.set_state(gstreamer::State::Playing).unwrap();

    button_window.show()?;
    video_window2.show()?;
    video_window1.run()?;

    Ok(())
}
