//! Generic editors for values and types, shared by panel widgets, constants and context menus.

use crate::theme;
use eframe::egui::{self, Color32, Ui};
use labcraft_core::{ArrayVal, DataType, Field, NumKind, Value};

/// Editor height used for layout of nested items, matching `Widget::panel_size`.
pub fn edit_value(ui: &mut Ui, ty: &DataType, v: &mut Value, editable: bool) -> bool {
    let col = theme::type_color(ty);
    let mut changed = false;
    match (ty, v) {
        (DataType::Num(k), v) => {
            if editable {
                changed |= match v {
                    Value::Int(i) => {
                        let (lo, hi) = k.int_range();
                        ui.add(egui::DragValue::new(i).range(lo..=hi)).changed()
                    }
                    Value::Dbl(d) => ui.add(egui::DragValue::new(d).speed(0.05)).changed(),
                    _ => false,
                };
            } else {
                ui.colored_label(col, format!("{v}"));
            }
        }
        (DataType::Bool, Value::Bool(b)) => {
            if editable {
                changed |= ui.checkbox(b, if *b { "TRUE" } else { "FALSE" }).changed();
            } else {
                ui.colored_label(col, if *b { "TRUE" } else { "FALSE" });
            }
        }
        (DataType::Str, Value::Str(s)) => {
            if editable {
                changed |= ui.add(egui::TextEdit::singleline(s).desired_width(110.0)).changed();
            } else {
                ui.colored_label(col, s.as_str());
            }
        }
        (DataType::Enum(items), Value::Int(i)) => {
            let cur = items.get(*i as usize).cloned().unwrap_or_default();
            if editable {
                egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(cur).show_ui(ui, |ui| {
                    for (n, name) in items.iter().enumerate() {
                        changed |= ui.selectable_value(i, n as i64, name).changed();
                    }
                });
            } else {
                ui.colored_label(col, cur);
            }
        }
        (DataType::Array(elem, dims), Value::Array(a)) => {
            changed |= edit_array(ui, elem, *dims, a, editable);
        }
        (DataType::Cluster(fields), Value::Cluster(vals)) => {
            ui.vertical(|ui| {
                for (f, v) in fields.iter().zip(vals.iter_mut()) {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&f.name).color(Color32::from_gray(185)).size(11.0));
                        changed |= edit_value(ui, &f.ty, v, editable);
                    });
                }
            });
        }
        _ => {
            ui.colored_label(Color32::LIGHT_RED, "type mismatch");
        }
    }
    changed
}

fn edit_array(ui: &mut Ui, elem: &DataType, dims: usize, a: &mut ArrayVal, editable: bool) -> bool {
    let mut changed = false;
    if a.dims.len() != dims {
        a.dims = vec![0; dims];
        a.data.clear();
    }
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (d, n) in a.dims.clone().iter().enumerate() {
                ui.label(egui::RichText::new(format!("dim{d}")).size(10.0).color(theme::TEXT_DIM));
                let mut size = *n;
                if editable && ui.add(egui::DragValue::new(&mut size).range(0..=4096)).changed() {
                    resize_array(a, elem, d, size);
                    changed = true;
                } else if !editable {
                    ui.label(format!("{n}"));
                }
            }
        });
        let cols = if dims > 1 { a.dims[dims - 1].max(1) } else { 6 };
        egui::ScrollArea::both().max_height(if dims > 1 { 90.0 } else { 44.0 }).id_salt(ui.next_auto_id()).show(ui, |ui| {
            let n = a.data.len();
            egui::Grid::new(ui.next_auto_id()).spacing([4.0, 2.0]).show(ui, |ui| {
                for (i, v) in a.data.iter_mut().enumerate() {
                    changed |= edit_value(ui, elem, v, editable);
                    if (i + 1) % cols == 0 && i + 1 < n {
                        ui.end_row();
                    }
                }
            });
        });
    });
    changed
}

/// Resize dimension `d` of a rectangular array, keeping existing elements where they overlap.
fn resize_array(a: &mut ArrayVal, elem: &DataType, d: usize, size: usize) {
    let mut new_dims = a.dims.clone();
    new_dims[d] = size;
    let total: usize = new_dims.iter().product();
    let (so, sn) = (ArrayVal::strides(&a.dims), ArrayVal::strides(&new_dims));
    let mut data = vec![Value::default_for(elem); total];
    let old_total: usize = a.dims.iter().product();
    for flat in 0..old_total {
        let (mut rest, mut dst, mut ok) = (flat, 0, true);
        for k in 0..a.dims.len() {
            let idx = rest / so[k];
            rest %= so[k];
            if idx >= new_dims[k] {
                ok = false;
                break;
            }
            dst += idx * sn[k];
        }
        if ok {
            data[dst] = a.data[flat].clone();
        }
    }
    a.dims = new_dims;
    a.data = data;
}

fn simple_types() -> Vec<(&'static str, DataType)> {
    vec![
        ("Numeric (DBL)", DataType::DBL),
        ("Integer (I32)", DataType::I32),
        ("Boolean", DataType::Bool),
        ("String", DataType::Str),
        ("Enum", DataType::Enum(vec!["Item 0".into(), "Item 1".into(), "Item 2".into()])),
        ("Array of DBL", DataType::arr_dbl()),
        ("Cluster", DataType::Cluster(vec![Field { name: "a".into(), ty: DataType::DBL }, Field { name: "b".into(), ty: DataType::Bool }])),
    ]
}

/// Menu for editing a type in place: numeric representation, array dimensions and element type,
/// enum items, and cluster elements. Returns true if the type changed.
pub fn type_menu(ui: &mut Ui, ty: &mut DataType) -> bool {
    let mut changed = false;
    match ty {
        DataType::Num(k) => {
            ui.menu_button(format!("Representation ({})", k.label()), |ui| {
                for cand in NumKind::ALL {
                    if ui.selectable_label(*k == cand, cand.label()).clicked() {
                        *k = cand;
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        DataType::Array(elem, dims) => {
            ui.horizontal(|ui| {
                ui.label("Dimensions");
                let mut d = *dims;
                if ui.add(egui::DragValue::new(&mut d).range(1..=4)).changed() {
                    *dims = d;
                    changed = true;
                }
            });
            ui.menu_button("Element type", |ui| {
                changed |= type_menu(ui, elem);
            });
        }
        DataType::Enum(items) => {
            ui.label("Items (one per line)");
            let mut text = items.join("\n");
            if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(4).desired_width(140.0)).changed() {
                *items = text.lines().map(|l| l.to_string()).collect();
                if items.is_empty() {
                    items.push("Item 0".into());
                }
                changed = true;
            }
        }
        DataType::Cluster(fields) => {
            let mut remove = None;
            for (i, f) in fields.iter_mut().enumerate() {
                ui.menu_button(format!("{} : {}", f.name, f.ty.label()), |ui| {
                    if ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(100.0)).changed() {
                        changed = true;
                    }
                    changed |= type_menu(ui, &mut f.ty);
                    ui.menu_button("Change type", |ui| {
                        for (label, t) in simple_types() {
                            if ui.button(label).clicked() {
                                f.ty = t;
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                    if ui.button("Remove element").clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            }
            if let Some(i) = remove {
                fields.remove(i);
                changed = true;
            }
            ui.menu_button("Add element", |ui| {
                for (label, t) in simple_types() {
                    if ui.button(label).clicked() {
                        let name = format!("element {}", fields.len());
                        fields.push(Field { name, ty: t });
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        DataType::Bool | DataType::Str => {}
    }
    changed
}
