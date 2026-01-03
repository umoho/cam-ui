use eframe::egui;
use parking_lot::Mutex;
use std::sync::Arc;

pub struct CameraApp {
    frame_buffer: Arc<Mutex<Option<egui::ColorImage>>>,
    texture: Option<egui::TextureHandle>,
    iso: u32,
    shutter: String,
}

impl CameraApp {
    pub fn new(frame_buffer: Arc<Mutex<Option<egui::ColorImage>>>) -> Self {
        Self {
            frame_buffer,
            texture: None,
            iso: 800,
            shutter: "1/500".to_string(),
        }
    }
}

impl eframe::App for CameraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 获取最新图像并转换为 GPU 纹理
        if let Some(image) = self.frame_buffer.lock().take() {
            self.texture = Some(ctx.load_texture("cam_frame", image, Default::default()));
        }

        // 2. 全屏背景绘制
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let rect = ui.max_rect();

                // 绘制背景图
                if let Some(texture) = &self.texture {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // 3. 叠加 UI：顶部栏
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add_space(20.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new("● LIVE")
                                .color(egui::Color32::RED)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(20.0);
                            // 渲染 SVG 图标
                            ui.add(
                                egui::Image::new(crate::icons::ICON_SETTINGS)
                                    .tint(egui::Color32::WHITE)
                                    .max_width(24.0),
                            );
                        });
                    });
                });

                // 4. 叠加 UI：底部参数区
                let bottom_bar_height = 80.0;
                let bottom_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.max.y - bottom_bar_height),
                    rect.max,
                );

                // 绘制半透明背景
                ui.painter()
                    .rect_filled(bottom_rect, 0.0, egui::Color32::from_black_alpha(180));

                #[allow(deprecated)]
                ui.allocate_ui_at_rect(bottom_rect, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(40.0);
                        param_widget(ui, "ISO", &self.iso.to_string());
                        ui.add_space(60.0);
                        param_widget(ui, "SHUTTER", &self.shutter);
                    });
                });
            });

        // 关键：请求下一帧重绘（实现实时视频）
        ctx.request_repaint();
    }
}

fn param_widget(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(10.0)
                .color(egui::Color32::LIGHT_GRAY),
        );
        ui.label(
            egui::RichText::new(value)
                .size(24.0)
                .strong()
                .color(egui::Color32::WHITE),
        );
    });
}
