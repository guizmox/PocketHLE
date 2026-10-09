//! Code-native PocketPC housing. A single transform rotates shell, LCD and hitboxes.
use eframe::egui::{self,Color32,Pos2,Rect,Vec2,Stroke};
use pocket_library::{RotationPref,GuestButton};
#[derive(Clone,Copy)]
pub struct PocketPcLayout {
    pub origin: Pos2,
    pub scale: f32,
    pub turns: u8,
    pub portrait: Vec2,
}
impl PocketPcLayout {
    pub fn new(native:[u32;2],rotation:RotationPref,scale:f32,origin:Pos2)->Self {
        let landscape=native[0]>native[1];
        let screen=if landscape {Vec2::new(native[1] as f32,native[0] as f32)}
            else {Vec2::new(native[0] as f32,native[1] as f32)};
        let turns=(u8::from(landscape)+match rotation {
            RotationPref::None=>0,RotationPref::Cw90=>1,RotationPref::Half=>2,RotationPref::Ccw90=>3,
        })%4;
        Self{origin,scale,turns,portrait:screen+Vec2::new(48.0,188.0)}
    }
    pub fn size(self)->Vec2 {
        let s=self.portrait*self.scale;
        if self.turns%2==1 {Vec2::new(s.y,s.x)} else {s}
    }
    pub fn point(self,x:f32,y:f32)->Pos2 {
        let w=self.portrait.x;let h=self.portrait.y;
        let p=match self.turns {
            0=>Vec2::new(x,y),1=>Vec2::new(h-y,x),2=>Vec2::new(w-x,h-y),_=>Vec2::new(y,w-x),
        };
        self.origin+p*self.scale
    }
    pub fn rect(self,x:f32,y:f32,w:f32,h:f32)->Rect {
        Rect::from_two_pos(self.point(x,y),self.point(x+w,y+h))
    }
    pub fn screen(self)->Rect {
        self.rect(24.0,56.0,self.portrait.x-48.0,self.portrait.y-188.0)
    }
    pub fn controls(self)->Vec<(Rect,&'static str,GuestButton)> {
        let cx=self.portrait.x/2.0;let y=self.portrait.y-132.0;
        [(cx-14.0,y+14.0,28.0,25.0,"▲",GuestButton::DpadUp),
         (cx-14.0,y+77.0,28.0,25.0,"▼",GuestButton::DpadDown),
         (cx-44.0,y+42.0,25.0,30.0,"◀",GuestButton::DpadLeft),
         (cx+19.0,y+42.0,25.0,30.0,"▶",GuestButton::DpadRight),
         (cx-15.0,y+42.0,30.0,30.0,"OK",GuestButton::Action),
         (24.0,y+26.0,42.0,27.0,"A",GuestButton::ButtonA),
         (24.0,y+66.0,42.0,27.0,"S1",GuestButton::Soft1),
         (self.portrait.x-66.0,y+26.0,42.0,27.0,"B",GuestButton::ButtonB),
         (self.portrait.x-66.0,y+66.0,42.0,27.0,"C",GuestButton::ButtonC),
         (cx-76.0,y+109.0,50.0,17.0,"S2",GuestButton::Soft2),
         (cx+26.0,y+109.0,50.0,17.0,"T",GuestButton::Turbo)]
         .into_iter().map(|(x,y,w,h,label,button)|(self.rect(x,y,w,h),label,button)).collect()
    }
    fn text(self,painter:&egui::Painter,x:f32,y:f32,text:&str,size:f32,color:Color32) {
        let galley=painter.layout_no_wrap(text.into(),egui::FontId::proportional(size*self.scale),color);
        let top_left=self.point(x-galley.size().x/self.scale/2.0,y-galley.size().y/self.scale/2.0);
        painter.add(egui::epaint::TextShape::new(top_left,galley,color)
            .with_angle(self.turns as f32*std::f32::consts::FRAC_PI_2));
    }
    pub fn draw_shell(self,painter:&egui::Painter) {
        let w=self.portrait.x;let h=self.portrait.y;let s=self.scale;
        let body=Rect::from_min_size(self.origin,self.size());
        painter.rect_filled(body.translate(Vec2::new(3.0,5.0)*s),24.0*s,Color32::from_black_alpha(90));
        painter.rect_filled(body,23.0*s,Color32::from_rgb(78,88,98));
        painter.rect_filled(self.rect(2.0,2.0,w-4.0,h-4.0),22.0*s,Color32::from_rgb(218,223,228));
        painter.rect_filled(self.rect(7.0,7.0,w-14.0,h-14.0),18.0*s,Color32::from_rgb(160,172,184));
        painter.rect_filled(self.rect(11.0,10.0,w-22.0,h-20.0),16.0*s,Color32::from_rgb(197,205,212));
        painter.rect_stroke(body.shrink(2.0*s),22.0*s,Stroke::new(s,Color32::from_rgb(241,245,248)));
        // Speaker, status light and engraved branding stay attached to the shell.
        for row in 0..3 {for col in 0..11 {
            painter.circle_filled(self.point(w/2.0-25.0+col as f32*5.0,17.0+row as f32*4.0),0.9*s,Color32::from_rgb(65,77,87));
        }}
        painter.circle_filled(self.point(w-28.0,22.0),2.8*s,Color32::from_rgb(86,190,158));
        self.text(painter,w/2.0,41.0,"PocketHLE",11.0,Color32::from_rgb(68,81,92));
        let lcd=self.screen();
        painter.rect_filled(lcd.expand(9.0*s),6.0*s,Color32::from_rgb(112,125,137));
        painter.rect_filled(lcd.expand(5.0*s),3.0*s,Color32::from_rgb(34,44,52));
        painter.rect_stroke(lcd.expand(6.0*s),4.0*s,Stroke::new(s,Color32::from_rgb(237,241,243)));
        painter.rect_filled(lcd,0.0,Color32::BLACK);
        let y=h-132.0;
        painter.circle_filled(self.point(w/2.0,y+58.0),48.0*s,Color32::from_rgb(117,130,143));
        painter.circle_filled(self.point(w/2.0,y+58.0),44.0*s,Color32::from_rgb(53,65,77));
        painter.circle_stroke(self.point(w/2.0,y+58.0),45.0*s,Stroke::new(s,Color32::from_rgb(233,239,243)));
    }
    pub fn draw_button(self,painter:&egui::Painter,rect:Rect,label:&str,pressed:bool) {
        let is_nav=matches!(label,"▲"|"▼"|"◀"|"▶");
        if !is_nav {
            painter.rect_filled(rect,5.0*self.scale,if pressed {Color32::from_rgb(58,136,184)} else {Color32::from_rgb(135,148,160)});
            painter.rect_stroke(rect,5.0*self.scale,Stroke::new(self.scale,Color32::from_rgb(231,237,241)));
        } else if pressed {
            painter.rect_filled(rect,5.0*self.scale,Color32::from_rgb(58,136,184));
        }
        if is_nav {
            let direction=match label {"▲"=>0.0,"▶"=>1.0,"▼"=>2.0,_=>3.0};
            let angle=(self.turns as f32+direction)*std::f32::consts::FRAC_PI_2;
            let points=[Vec2::new(-5.0,3.5),Vec2::new(0.0,-4.5),Vec2::new(5.0,3.5)]
                .into_iter().map(|v|rect.center()+Vec2::new(angle.cos()*v.x-angle.sin()*v.y,
                    angle.sin()*v.x+angle.cos()*v.y)*self.scale).collect();
            painter.add(egui::Shape::convex_polygon(points,Color32::from_rgb(245,248,250),Stroke::NONE));
            return;
        }
        // Rotate legends around their own centers, exactly like the hardware.
        let galley=painter.layout_no_wrap(label.into(),egui::FontId::proportional(
            if is_nav {13.0*self.scale} else {11.0*self.scale}),Color32::from_rgb(245,248,250));
        let angle=self.turns as f32*std::f32::consts::FRAC_PI_2;
        let v=galley.size()*0.5;let offset=Vec2::new(angle.cos()*v.x-angle.sin()*v.y,angle.sin()*v.x+angle.cos()*v.y);
        painter.add(egui::epaint::TextShape::new(rect.center()-offset,galley,Color32::WHITE).with_angle(angle));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn screen_and_controls_rotate_as_one_layout() {
        for native in [[240,320],[320,240],[480,800],[800,480]] {
            for rotation in RotationPref::ALL {
                let layout=PocketPcLayout::new(native,rotation,2.0/1.25,Pos2::ZERO);
                let screen=layout.screen();let size=if rotation.is_quarter_turn() {[native[1],native[0]]} else {native};
                let pixels=screen.size()*1.25;
                assert_eq!([pixels.x.round() as u32,pixels.y.round() as u32],[size[0]*2,size[1]*2]);
                let body=Rect::from_min_size(Pos2::ZERO,layout.size());
                assert!(body.contains_rect(screen));
                let controls=layout.controls();assert_eq!(controls.len(),11);
                for (i,(button,_,_)) in controls.iter().enumerate() {
                    assert!(body.contains_rect(*button));assert!(!screen.intersects(*button));
                    for (other,_,_) in controls.iter().skip(i+1) {assert!(!button.intersects(*other));}
                }
            }
        }
    }
}
