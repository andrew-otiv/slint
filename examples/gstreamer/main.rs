// Copyright © SixtyFPS GmbH <info@slint-ui.com>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-commercial

use gstreamer::prelude::*;
use gstreamer_gl::prelude::*;
use gstreamer_gl::GLVideoFrameExt;
use gstreamer_video::VideoFrameExt;

slint::include_modules!();

struct Player<C: slint::ComponentHandle + 'static> {
    app: slint::Weak<C>,
    pipeline: gstreamer::Element,
    appsink: gstreamer_app::AppSink,
    current_sample: std::sync::Arc<std::sync::Mutex<Option<gstreamer::Sample>>>,
    gst_gl_context: Option<gstreamer_gl::GLContext>,
}

impl<C: slint::ComponentHandle + 'static> Player<C> {
    fn new(app: slint::Weak<C>) -> Result<Self, anyhow::Error> {
        gstreamer::init()?;

        let source = gstreamer::ElementFactory::make("playbin").build()?;
        source.set_property("uri", "https://www.freedesktop.org/software/gstreamer-sdk/data/media/sintel_trailer-480p.webm");

        let appsink = gstreamer::ElementFactory::make("appsink")
            .build()?
            .dynamic_cast::<gstreamer_app::AppSink>()
            .unwrap();

        appsink.set_property("enable-last-sample", false);
        appsink.set_property("emit-signals", false);
        appsink.set_property("max-buffers", 1u32);

        let caps = gstreamer::Caps::builder("video/x-raw")
            .features([gstreamer_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
            .field("format", gstreamer_video::VideoFormat::Rgba.to_str())
            .field("texture-target", "2D")
            .build();
        appsink.set_caps(Some(&caps));

        let glsink = gstreamer::ElementFactory::make("glsinkbin").build()?;
        glsink.set_property("sink", &appsink);

        source.set_property("video-sink", &glsink);

        Ok(Self {
            app,
            pipeline: source,
            appsink,
            current_sample: std::sync::Arc::new(std::sync::Mutex::new(None)),
            gst_gl_context: None,
        })
    }

    fn setup_graphics(&mut self, graphics_api: &slint::GraphicsAPI) {
        let egl = match graphics_api {
            slint::GraphicsAPI::NativeOpenGL { get_proc_address } => {
                glutin_egl_sys::egl::Egl::load_with(|symbol| {
                    get_proc_address(&std::ffi::CString::new(symbol).unwrap())
                })
            }
            _ => panic!("unsupported graphics API"),
        };

        let (gst_gl_context, gst_gl_display) = unsafe {
            let platform = gstreamer_gl::GLPlatform::EGL;

            let egl_display = egl.GetCurrentDisplay();
            let display =
                gstreamer_gl_egl::GLDisplayEGL::with_egl_display(egl_display as usize).unwrap();
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

        self.gst_gl_context = Some(gst_gl_context.clone());

        let bus = self.pipeline.bus().unwrap();
        bus.set_sync_handler({
            let gst_gl_context = gst_gl_context.clone();
            move |_, msg| {
                match msg.view() {
                    gstreamer::MessageView::NeedContext(ctx) => {
                        let ctx_type = ctx.context_type();
                        if ctx_type == *gstreamer_gl::GL_DISPLAY_CONTEXT_TYPE {
                            if let Some(element) = msg.src().map(|source| {
                                source.clone().downcast::<gstreamer::Element>().unwrap()
                            }) {
                                let gst_context = gstreamer::Context::new(ctx_type, true);
                                gst_context.set_gl_display(&gst_gl_display);
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
                                    structure.set("context", &gst_gl_context);
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

        self.pipeline.set_state(gstreamer::State::Playing).unwrap();

        let app_weak = self.app.clone();

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

pub fn main() -> Result<(), anyhow::Error> {
    let video_window1 = MainWindow::new()?;
    let video_window2 = MainWindow::new()?;
    let button_window = OtherWindow::new()?;
    let video_window1_clone = video_window1.clone_strong();
    let video_window2_clone = video_window2.clone_strong();

    // let mut player1 = Player::new(video_window1.as_weak())?;
    // let mut player2 = Player::new(video_window2.as_weak())?;

    fn set_rendering_notifier(video_window: MainWindow, // , player: &mut Player<MainWindow>
    ) {
        // let video_window = (*video_window).clone_strong();
        // let player = player.clone_mut();
        let video_window_weak = video_window.as_weak();
        let video_window_weak2 = video_window.as_weak();
        let mut player = Player::new(video_window_weak).unwrap();
        let video_window_window = video_window.window();
        video_window_window
            .set_rendering_notifier(move |state, graphics_api| match state {
                slint::RenderingState::RenderingSetup => {
                    player.setup_graphics(graphics_api);
                }
                slint::RenderingState::RenderingTeardown => {
                    todo!()
                }
                slint::RenderingState::BeforeRendering => {
                    if let Some(sample) = player.current_sample.lock().unwrap().as_ref() {
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
                            video_window_weak2
                                .upgrade()
                                .unwrap()
                                .global::<MainCameraAdapter>()
                                .set_video_frame(image.clone())
                        }
                    }
                }
                _ => {}
            })
            .unwrap();
    }
    set_rendering_notifier(video_window1_clone);
    set_rendering_notifier(video_window2_clone);

    button_window.show()?;
    video_window2.show()?;
    video_window1.run()?;

    Ok(())
}
