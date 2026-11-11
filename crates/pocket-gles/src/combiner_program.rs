//! Binary layout of the GoForce AR10 and AR11 combiner programs.
//!
//! Decoding a program describes its fields. Execution and capability
//! advertisement belong to the renderer and its guest API respectively.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    Ar10,
    Ar11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    UnknownArchitecture,
    InvalidStageCount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub alpha_output: u16,
    pub alpha_inputs: u32,
    pub flags: u8,
    pub color_output: u16,
    pub color_input_modifiers: u16,
    pub color_inputs: u32,
    pub color_input_modes: u16,
    pub texture_routing: u32,
}

/// An input as represented in the driver's normalized operand layout.
/// Modifier values are preserved separately from the source selector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Operand {
    pub selector: u8,
    pub components: [u8; 3],
    pub modifiers: [u8; 3],
}

impl Operand {
    fn register(raw: u8, components: [u8; 3], modifiers: [u8; 3]) -> Self {
        let family = (raw >> 1) & 0x60;
        let selector = if family <= 0x20 {
            family | (raw & 3)
        } else {
            family
        };
        Self {
            selector,
            components: if matches!(family, 0x40 | 0x60) {
                components
            } else {
                [0; 3]
            },
            modifiers,
        }
    }

    fn literal(components: [u8; 3]) -> Self {
        Self {
            selector: 0x80,
            components,
            modifiers: [0; 3],
        }
    }
}

/// Encoded result routing for an active color or alpha calculation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub destination: u8,
    pub modifiers: [bool; 2],
}

fn output(word: u16, destination: u8, shift: u8) -> Output {
    Output {
        destination,
        modifiers: [word & (1 << shift) != 0, word & (2 << shift) != 0],
    }
}

impl Stage {
    /// This is an operation selector. Values below five identify texture
    /// units, while the remaining values have other meanings.
    pub fn texture_operation(self) -> u8 {
        (self.texture_routing >> 30) as u8 | ((self.flags >> 3) & 4)
    }

    pub fn color_mode(self) -> u8 {
        if self.color_output & 0x8000 != 0 {
            3
        } else {
            (self.color_output >> 13) as u8
        }
    }

    pub fn alpha_mode(self) -> u8 {
        self.flags >> 6
    }

    pub fn color_outputs(self) -> [Output; 2] {
        let word = self.color_output;
        match self.color_mode() {
            3 => [
                output(word, ((word >> 13) & 3) as u8, 11),
                Output::default(),
            ],
            2 => [
                output(word, ((word >> 9) & 3) as u8, 11),
                output(word, ((word >> 4) & 3) as u8, 7),
            ],
            _ => [Output::default(); 2],
        }
    }

    pub fn alpha_outputs(self) -> [Output; 2] {
        let word = self.alpha_output;
        let primary = (word >> 13) as u8;
        match self.alpha_mode() {
            3 => [output(word, primary, 11), Output::default()],
            2 => {
                // The secondary destination is numbered with the primary omitted.
                let secondary = ((word >> 9) & 3) as u8;
                [
                    output(word, primary, 11),
                    output(word, secondary + u8::from(secondary >= primary), 7),
                ]
            }
            _ => [Output::default(); 2],
        }
    }

    pub fn color_operands(self) -> [Operand; 4] {
        if self.color_mode() == 0 {
            return [Operand::default(); 4];
        }
        std::array::from_fn(|slot| {
            let shift = 3 - slot;
            let raw = (self.color_inputs >> (8 * shift)) as u8;
            let mode = (self.color_input_modes >> (4 * shift)) as u8 & 15;
            let modifiers = (self.color_input_modifiers >> (4 * shift)) as u8 & 15;
            if mode & 8 != 0 {
                Operand::register(
                    raw,
                    [raw & 3, (raw >> 2) & 3, (raw >> 4) & 3],
                    [modifiers & 3, (modifiers >> 2) & 1, modifiers >> 3],
                )
            } else {
                let code = u16::from(mode) << 12 | u16::from(raw) << 4 | u16::from(modifiers);
                Operand::literal([
                    (code & 31) as u8,
                    ((code >> 5) & 31) as u8,
                    (code >> 10) as u8,
                ])
            }
        })
    }

    pub fn alpha_operands(self) -> [Operand; 4] {
        if self.alpha_mode() == 0 {
            return [Operand::default(); 4];
        }
        std::array::from_fn(|slot| {
            let shift = 3 - slot;
            let raw = (self.alpha_inputs >> (8 * shift)) as u8;
            if self.flags & (1 << shift) != 0 {
                Operand::register(
                    raw,
                    [raw & 3, 0, 0],
                    [(raw >> 2) & 3, (raw >> 4) & 1, (raw >> 5) & 1],
                )
            } else {
                Operand::literal([raw & 31, 0, 0])
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub architecture: Architecture,
    pub flags: u8,
    pub stages: Vec<Stage>,
}

impl Program {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let header = bytes.get(..5).ok_or(DecodeError::Truncated)?;
        let architecture = match &header[..4] {
            b"AR10" => Architecture::Ar10,
            b"AR11" => Architecture::Ar11,
            _ => return Err(DecodeError::UnknownArchitecture),
        };
        let count = usize::from(header[4] & 0x3f);
        // The loader caps both formats at twelve records. AR10's
        // architecture validator further limits its programs to four.
        let limit = match architecture {
            Architecture::Ar10 => 4,
            Architecture::Ar11 => 12,
        };
        if count == 0 || count > limit {
            return Err(DecodeError::InvalidStageCount);
        }

        let records = bytes.get(5..5 + count * 21).ok_or(DecodeError::Truncated)?;
        let stages = records
            .chunks_exact(21)
            .map(|stage| {
                let half = |i| u16::from_le_bytes([stage[i], stage[i + 1]]);
                let word =
                    |i| u32::from_le_bytes([stage[i], stage[i + 1], stage[i + 2], stage[i + 3]]);
                Stage {
                    alpha_output: half(0),
                    alpha_inputs: word(2),
                    flags: stage[6],
                    color_output: half(7),
                    color_input_modifiers: half(9),
                    color_inputs: word(11),
                    color_input_modes: half(15),
                    texture_routing: word(17),
                }
            })
            .collect();

        Ok(Self {
            architecture,
            flags: header[4] >> 6,
            stages,
        })
    }
}

/// Values supplied to the scalar combiner independently of texture sampling.
pub struct Inputs {
    pub textures: [[f32; 4]; 5],
    pub primary: [f32; 4],
    pub constants: [[f32; 4]; 4],
}

/// A decoded operation whose arithmetic or sampling semantics are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedOperation;

#[derive(Clone, Copy, Debug)]
enum Source {
    Register(usize),
    Primary(usize),
    Interpolant { unit: usize, component: usize },
    Constant(usize, bool),
    Texture(usize),
    Literal(f32),
}

#[derive(Clone, Copy, Debug)]
struct InputTerm {
    source: Source,
    scale: f32,
    bias: f32,
}

#[derive(Debug)]
struct Calculation {
    register: usize,
    channel: usize,
    terms: [InputTerm; 4],
    mode: CalculationMode,
    clamp: [bool; 2],
    compare_register: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum CalculationMode {
    Sum,
    Maximum,
    KillEqual,
    KillNotEqual,
}

const ZERO_TERM: InputTerm = InputTerm {
    source: Source::Literal(0.),
    scale: 1.,
    bias: 0.,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureRoute {
    pub texture: usize,
    pub s: usize,
    pub t: usize,
    /// Component bank: selectors 0..7 read x, selectors 8..15 read y.
    pub s_component: usize,
    pub t_component: usize,
}

const MAX_PROGRAM_STAGES: usize = 12;
const TEXTURE_ALPHA_DESTINATION: u8 = 4;

#[derive(Clone, Copy, Debug)]
enum Fetch {
    Keep,
    Texture(usize),
    Framebuffer,
}

#[derive(Debug)]
struct CompiledStage {
    fetch: Fetch,
    calculations: Vec<Calculation>,
}

#[derive(Debug)]
pub struct BasicProgram {
    id: u64,
    stages: Vec<CompiledStage>,
    texture_units: u8,
    routes: Vec<TextureRoute>,
    coordinate_units: u8,
    reads_framebuffer: bool,
    framebuffer_reads: u8,
    output_register: usize,
    requires_sc15: bool,
}

impl Program {
    /// Resolve supported sources and arithmetic before evaluating any pixels.
    pub fn compile_basic(&self) -> Result<BasicProgram, UnsupportedOperation> {
        let ar11 = self.architecture == Architecture::Ar11;
        let output_register = usize::from(self.flags);
        let registers = match self.architecture {
            Architecture::Ar10 => 2,
            Architecture::Ar11 => 4,
        };
        if output_register >= registers {
            return Err(UnsupportedOperation);
        }
        let mut stages = Vec::with_capacity(self.stages.len());
        let mut texture_units = 0;
        let mut coordinate_units = 0;
        let mut routes = Vec::new();
        let mut available_texel = 0u8;
        let mut requires_sc15 = false;
        for stage in &self.stages {
            let operation = stage.texture_operation();
            let unit = match operation {
                unit @ 0..=4 => Some(usize::from(unit)),
                5 | 7 => None,
                _ => return Err(UnsupportedOperation),
            };
            let coords = stage.texture_routing & 0x3ff;
            let s_unit = (coords >> 5) as usize;
            let t_selector = (coords & 31) as usize;
            let route = if let Some(texture) = unit {
                let coordinate_limit = if ar11 { 8 } else { 4 };
                if texture >= if ar11 { 5 } else { 4 }
                    || s_unit >= 16
                    || t_selector >= 16
                    || (s_unit & 7) >= coordinate_limit
                    || (t_selector & 7) >= coordinate_limit
                {
                    return Err(UnsupportedOperation);
                }
                Some(TextureRoute {
                    texture,
                    s: s_unit & 7,
                    t: t_selector & 7,
                    s_component: usize::from(s_unit >= 8),
                    t_component: usize::from(t_selector >= 8),
                })
            } else {
                if coords != 0x3ff {
                    return Err(UnsupportedOperation);
                }
                None
            };
            let color_routes = (stage.texture_routing >> 10) & 0xfffff;
            if !ar11 && !matches!(color_routes, 0xfffff | 0xa56d7) {
                return Err(UnsupportedOperation);
            }
            // A stage may read the fragment already in the render target
            // or reuse the texture register from the preceding stage.
            let fetch = if let Some(route) = route {
                let slot = match routes.iter().position(|&r| r == route) {
                    Some(slot) => slot,
                    None if routes.len() < MAX_PROGRAM_STAGES => {
                        routes.push(route);
                        routes.len() - 1
                    }
                    None => return Err(UnsupportedOperation),
                };
                texture_units |= 1 << route.texture;
                coordinate_units |= (1 << route.s) | (1 << route.t);
                Fetch::Texture(slot)
            } else if operation == 5 {
                Fetch::Framebuffer
            } else {
                Fetch::Keep
            };
            if !matches!(fetch, Fetch::Keep) {
                available_texel = 0xf;
            }
            let mut calculations = Vec::with_capacity(4);
            // GoForce makes the co-issued alpha result available to RGB.
            for (alpha, mode, operands, outputs) in [
                (
                    true,
                    stage.alpha_mode(),
                    stage.alpha_operands(),
                    stage.alpha_outputs(),
                ),
                (
                    false,
                    stage.color_mode(),
                    stage.color_operands(),
                    stage.color_outputs(),
                ),
            ] {
                if mode == 0 {
                    continue;
                }
                let word = if alpha {
                    stage.alpha_output
                } else {
                    stage.color_output
                };
                let paired = mode == 2;
                let kill = ar11 && mode == 1 && matches!(word & 0x7f, 0x70 | 0x71);
                let compare_positive = alpha && word & 0x01ff == 0x0121;
                let maximum = ar11 && mode == 3 && (word & 0x01ff == 0x0071 || compare_positive);
                if !kill && !paired && !maximum && (mode != 3 || word & 0x7f != 0x70) {
                    return Err(UnsupportedOperation);
                }
                let compare_register = (maximum && compare_positive).then(|| {
                    let first = outputs[0].destination;
                    let other = ((word >> 9) & 3) as u8;
                    usize::from(other + u8::from(other >= first))
                });
                if compare_register.is_some_and(|index| index >= registers) {
                    return Err(UnsupportedOperation);
                }
                let outputs = &outputs[..if paired { 2 } else { 1 }];
                for output in outputs {
                    let texture_alpha = alpha && output.destination == TEXTURE_ALPHA_DESTINATION;
                    if (!ar11 && output.modifiers != [false; 2])
                        || (output.destination >= 4 && !texture_alpha)
                    {
                        return Err(UnsupportedOperation);
                    }
                }
                for channel in if alpha { 3..4 } else { 0..3 } {
                    let [a, b, c, d] = operands.map(|operand| {
                        let mut term = compile_term(
                            operand,
                            available_texel,
                            if alpha { 0 } else { channel },
                            ar11,
                        )?;
                        if let Source::Primary(component) = term.source {
                            // RGB and alpha may read independently routed components
                            // of the interpolated color register. Ignore unused lanes.
                            requires_sc15 |= !matches!(color_routes, 0xfffff | 0xa56d7);
                            term.source = match color_routes {
                                0xfffff | 0xa56d7 => term.source,
                                _ => match (color_routes >> ((3 - component) * 5)) & 31 {
                                    selector @ 0..=15 => {
                                        let unit = (selector & 7) as usize;
                                        coordinate_units |= 1 << unit;
                                        Source::Interpolant {
                                            unit,
                                            component: (selector >> 3) as usize,
                                        }
                                    }
                                    selector @ 20..=23 => Source::Primary((selector - 20) as usize),
                                    _ => return Err(UnsupportedOperation),
                                },
                            };
                        }
                        Ok(term)
                    });
                    let terms = [a?, b?, c?, d?];
                    if paired {
                        for (output, pair) in outputs.iter().zip(terms.chunks_exact(2)) {
                            calculations.push(Calculation {
                                register: usize::from(output.destination),
                                channel,
                                terms: [pair[0], pair[1], ZERO_TERM, ZERO_TERM],
                                mode: CalculationMode::Sum,
                                clamp: output.modifiers,
                                compare_register: None,
                            });
                        }
                    } else {
                        calculations.push(Calculation {
                            register: usize::from(outputs[0].destination),
                            channel,
                            terms,
                            mode: if kill {
                                if word & 1 == 0 {
                                    CalculationMode::KillEqual
                                } else {
                                    CalculationMode::KillNotEqual
                                }
                            } else if maximum {
                                CalculationMode::Maximum
                            } else {
                                CalculationMode::Sum
                            },
                            clamp: outputs[0].modifiers,
                            compare_register,
                        });
                    }
                }
                // Alpha runs before RGB. It can initialize tex.a even in
                // a stage that did not fetch a texel.
                if ar11
                    && alpha
                    && !kill
                    && outputs
                        .iter()
                        .any(|o| o.destination == TEXTURE_ALPHA_DESTINATION)
                {
                    requires_sc15 |= available_texel == 0;
                    available_texel |= 1 << 3;
                }
            }
            stages.push(CompiledStage {
                fetch,
                calculations,
            });
        }
        let reads_framebuffer = stages
            .iter()
            .any(|stage| matches!(stage.fetch, Fetch::Framebuffer));
        // Conservative channel dependencies: all framebuffer texel reads,
        // including discard predicates, remain hazards even if a later stage
        // overwrites their result. Registers cannot introduce a dependency
        // which was not already observed at their original texel read.
        let mut framebuffer_reads = 0;
        let mut framebuffer_texel = false;
        for stage in &stages {
            match stage.fetch {
                Fetch::Framebuffer => framebuffer_texel = true,
                Fetch::Texture(_) => framebuffer_texel = false,
                Fetch::Keep => {}
            }
            if framebuffer_texel {
                for calc in &stage.calculations {
                    for term in calc.terms {
                        if let Source::Texture(component) = term.source {
                            framebuffer_reads |= 1 << component;
                        }
                    }
                }
            }
        }
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Ok(BasicProgram {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            stages,
            texture_units,
            routes,
            coordinate_units,
            reads_framebuffer,
            framebuffer_reads,
            output_register,
            requires_sc15,
        })
    }

    pub fn evaluate_basic(&self, inputs: &Inputs) -> Result<[f32; 4], UnsupportedOperation> {
        Ok(self.compile_basic()?.evaluate(inputs))
    }
}

fn compile_term(
    operand: Operand,
    available_texel: u8,
    channel: usize,
    ar11: bool,
) -> Result<InputTerm, UnsupportedOperation> {
    let [mapping, double, negate] = operand.modifiers;
    if mapping > 1 && !ar11 {
        return Err(UnsupportedOperation);
    }
    let component = usize::from(operand.components[channel]);
    let source = match operand.selector {
        register @ 0..=3 if ar11 || register < 2 => Source::Register(usize::from(register)),
        index @ 0x20..=0x23 if ar11 => Source::Constant(usize::from(index - 0x20), false),
        index @ 0x20..=0x23 => Source::Constant(usize::from((index - 0x20) >> 1), index & 1 != 0),
        0x40 if component < 4 => Source::Primary(component),
        0x60 if component < 4 && available_texel & (1 << component) != 0 => {
            Source::Texture(component)
        }
        0x80 if component < if ar11 { 32 } else { 5 } => {
            let magnitude = (component & 15) as f32 * 0.25;
            Source::Literal(if component & 16 == 0 {
                magnitude
            } else {
                -magnitude
            })
        }
        _ => return Err(UnsupportedOperation),
    };
    let scale = f32::from(1 + double);
    Ok(InputTerm {
        source,
        scale: if negate == 0 { scale } else { -scale },
        bias: scale
            * match mapping {
                1 if negate != 0 => 1.,
                2 => 0.5,
                3 => -0.5,
                _ => 0.,
            },
    })
}

impl BasicProgram {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn fragment_function(&self) -> String {
        use std::fmt::Write as _;

        let mut glsl = String::from(
            "uniform vec4 nvConstants[4];\nvec4 nvCombiner(vec4 primary) {\n\
             vec4 r0=vec4(0.0),r1=r0,r2=r0,r3=r0,texel=r0;\n",
        );
        for stage in &self.stages {
            glsl.push_str("{\n");
            match stage.fetch {
                Fetch::Keep => {}
                Fetch::Framebuffer => {
                    glsl.push_str("texel=texture2D(logicDestination,gl_FragCoord.xy/logicSize);\n");
                }
                Fetch::Texture(slot) => {
                    let TextureRoute {
                        texture,
                        s,
                        t,
                        s_component,
                        t_component,
                    } = self.routes[slot];
                    let coordinate = |unit: usize| {
                        let bank = if unit < 4 {
                            "pointReplace"
                        } else {
                            "pointReplaceHigh"
                        };
                        let component = ["x", "y", "z", "w"][unit % 4];
                        format!("pointCoordinate(uv{unit},{bank}.{component})")
                    };
                    let uv = if s == t && s_component == 0 && t_component == 1 {
                        coordinate(s)
                    } else {
                        format!(
                            "vec2({}.{},{}.{})",
                            coordinate(s),
                            ["x", "y"][s_component],
                            coordinate(t),
                            ["x", "y"][t_component]
                        )
                    };
                    writeln!(
                        glsl,
                        "texel=sampleRange(sampler{texture},{uv},lodBias{texture},lodLimit{texture},lodRange{texture});",
                    )
                    .unwrap();
                }
            }
            glsl.push_str("vec4 p0=r0,p1=r1,p2=r2,p3=r3,previousTexel=texel;\n");
            for (index, calc) in stage.calculations.iter().enumerate() {
                let swizzle = |channel| ["r", "g", "b", "a"][channel];
                let channel = swizzle(calc.channel);
                let terms = calc.terms.map(|term| {
                    let source = match term.source {
                        Source::Register(index) => format!("p{index}.{channel}"),
                        Source::Primary(component) => format!("primary.{}", swizzle(component)),
                        Source::Interpolant { unit, component } => {
                            format!("uv{unit}.{}", ["x", "y"][component])
                        }
                        Source::Constant(index, alpha) => {
                            let component = if alpha { "a" } else { channel };
                            format!("nvConstants[{index}].{component}")
                        }
                        Source::Texture(component) => {
                            let texture = if calc.channel == 3 {
                                "previousTexel"
                            } else {
                                "texel"
                            };
                            format!("{texture}.{}", swizzle(component))
                        }
                        Source::Literal(value) => format!("{value:.3}"),
                    };
                    format!("({source}*{:.1}+({:.1}))", term.scale, term.bias)
                });
                let [a, b, c, d] = terms;
                writeln!(glsl, "float left{index}={a}*{b},right{index}={c}*{d};").unwrap();
                if matches!(
                    calc.mode,
                    CalculationMode::KillEqual | CalculationMode::KillNotEqual
                ) {
                    let op = if matches!(calc.mode, CalculationMode::KillEqual) {
                        "=="
                    } else {
                        "!="
                    };
                    writeln!(glsl, "if(left{index}{op}right{index})discard;").unwrap();
                    continue;
                }
                let result = match calc.mode {
                    CalculationMode::Sum => format!("left{index}+right{index}"),
                    CalculationMode::Maximum => format!("max(left{index},right{index})"),
                    CalculationMode::KillEqual | CalculationMode::KillNotEqual => unreachable!(),
                };
                let result = match calc.clamp {
                    [false, false] => result,
                    [true, false] => format!("clamp({result},-1.0,1.0)"),
                    [_, true] => format!("clamp({result},0.0,1.0)"),
                };
                if let Some(destination) = calc.compare_register {
                    writeln!(glsl, "r{destination}.{channel}=left{index}<right{index}?clamp(right{index},0.0,1.0):0.0;").unwrap();
                }
                if calc.register == usize::from(TEXTURE_ALPHA_DESTINATION) {
                    writeln!(glsl, "texel.a={result};").unwrap();
                } else {
                    writeln!(glsl, "r{}.{channel}={result};", calc.register).unwrap();
                }
            }
            glsl.push_str("}\n");
        }
        writeln!(glsl, "return r{};\n}}", self.output_register).unwrap();
        glsl
    }

    /// Stable semantic identity for opt-in cross-host diagnostics, not a GL name.
    pub fn diagnostic_fingerprint(&self) -> u64 {
        // Exclude the process-local id. Do not invoke shader generation here:
        // a decoded program may still be rejected by the texture-unit limit.
        let description = format!(
            "{:?}",
            (
                &self.stages,
                &self.routes,
                self.texture_units,
                self.coordinate_units,
                self.reads_framebuffer,
                self.output_register,
            )
        );
        diagnostic_fingerprint(description.as_bytes())
    }

    pub fn reads_framebuffer(&self) -> bool {
        self.reads_framebuffer
    }

    /// A snapshot taken before the draw remains valid when the draw cannot
    /// write any channel read by the program. Depth and blending still execute
    /// in native primitive order; only redundant framebuffer copies disappear.
    pub(crate) fn framebuffer_needs_primitive_order(&self, color_mask: [bool; 4]) -> bool {
        color_mask
            .into_iter()
            .enumerate()
            .any(|(channel, write)| write && self.framebuffer_reads & (1 << channel) != 0)
    }

    pub fn texture_units(&self) -> u8 {
        self.texture_units
    }

    /// Uses additional color routing or a texture-alpha result before any fetch.
    pub fn requires_sc15(&self) -> bool {
        self.requires_sc15
    }

    pub fn coordinate_units(&self) -> u8 {
        self.coordinate_units
    }

    pub fn routes(&self) -> &[TextureRoute] {
        &self.routes
    }

    pub fn supports_texture_units(&self, count: usize) -> bool {
        u32::from(self.coordinate_units | self.texture_units) >> count.min(8) == 0
    }

    pub fn evaluate(&self, inputs: &Inputs) -> [f32; 4] {
        self.evaluate_sampled(
            inputs.primary,
            inputs.constants,
            [0., 0., 0., 1.],
            |route| inputs.textures[route.texture],
        )
    }

    /// Color-only inspection helper. Rasterization uses `evaluate_fragment`
    /// so a rejected fragment cannot become a transparent-black write.
    /// Coordinate interpolants default to zero in this color-only helper.
    pub fn evaluate_sampled(
        &self,
        primary: [f32; 4],
        constants: [[f32; 4]; 4],
        framebuffer: [f32; 4],
        sample: impl FnMut(TextureRoute) -> [f32; 4],
    ) -> [f32; 4] {
        self.evaluate_fragment(primary, constants, framebuffer, |_, _| 0., sample)
            .unwrap_or([0.; 4])
    }

    pub fn evaluate_fragment(
        &self,
        primary: [f32; 4],
        constants: [[f32; 4]; 4],
        framebuffer: [f32; 4],
        interpolant: impl Fn(usize, usize) -> f32,
        mut sample: impl FnMut(TextureRoute) -> [f32; 4],
    ) -> Option<[f32; 4]> {
        let mut textures = [[0.0; 4]; MAX_PROGRAM_STAGES];
        for (&route, texel) in self.routes.iter().zip(&mut textures) {
            *texel = sample(route);
        }
        let mut registers = [[0.0; 4]; 4];
        let mut texel = [0.0; 4];
        for stage in &self.stages {
            match stage.fetch {
                Fetch::Keep => {}
                Fetch::Texture(slot) => texel = textures[slot],
                Fetch::Framebuffer => texel = framebuffer,
            }
            let previous = registers;
            let previous_texel = texel;
            for calc in &stage.calculations {
                let channel = calc.channel;
                let [a, b, c, d] = calc.terms.map(|term| {
                    let value = match term.source {
                        Source::Register(index) => previous[index][channel],
                        Source::Primary(component) => primary[component],
                        Source::Interpolant { unit, component } => interpolant(unit, component),
                        Source::Constant(index, alpha) => {
                            constants[index][if alpha { 3 } else { channel }]
                        }
                        Source::Texture(component) => {
                            if channel == 3 {
                                previous_texel[component]
                            } else {
                                texel[component]
                            }
                        }
                        Source::Literal(value) => value,
                    };
                    value * term.scale + term.bias
                });
                let left = a * b;
                let right = c * d;
                if matches!(calc.mode, CalculationMode::KillEqual) && left == right
                    || matches!(calc.mode, CalculationMode::KillNotEqual) && left != right
                {
                    return None;
                }
                if matches!(
                    calc.mode,
                    CalculationMode::KillEqual | CalculationMode::KillNotEqual
                ) {
                    continue;
                }
                let result = match calc.mode {
                    CalculationMode::Sum => left + right,
                    CalculationMode::Maximum => left.max(right),
                    CalculationMode::KillEqual | CalculationMode::KillNotEqual => unreachable!(),
                };
                let result = match calc.clamp {
                    [false, false] => result,
                    [true, false] => result.clamp(-1., 1.),
                    [_, true] => result.clamp(0., 1.),
                };
                if let Some(index) = calc.compare_register {
                    // The secondary alpha write selects the positive
                    // contribution when the right product wins.
                    registers[index][channel] = if left < right {
                        right.clamp(0., 1.)
                    } else {
                        0.
                    };
                }
                if calc.register == usize::from(TEXTURE_ALPHA_DESTINATION) {
                    texel[3] = result;
                } else {
                    registers[calc.register][channel] = result;
                }
            }
        }
        Some(registers[self.output_register])
    }
}

/// FNV-1a over bytes, independent of host word size and randomized hash seeds.
pub fn diagnostic_fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

/// GoForce program names, binding and per-context constants.
#[derive(Default)]
pub struct ProgramObjects {
    programs: std::collections::BTreeMap<u32, Option<std::sync::Arc<BasicProgram>>>,
    next: u32,
    bound: u32,
    retained: Option<std::sync::Arc<BasicProgram>>,
    enabled: bool,
    constants: [[f32; 4]; 4],
}

impl ProgramObjects {
    /// Transfer the shared program namespace at EGL switches, preserving each
    /// context's enable, binding, constants and default (name-zero) program.
    pub(crate) fn exchange_namespace(&mut self, group: &mut Self) {
        // A deletion in another sharing context removes the name, not an
        // existing binding. Keep that immutable body until this context rebinds.
        if let Some(program) = self.programs.get(&self.bound) {
            self.retained = program.clone();
        }
        let zero = self.programs.remove(&0);
        std::mem::swap(&mut self.programs, &mut group.programs);
        std::mem::swap(&mut self.next, &mut group.next);
        if let Some(zero) = zero {
            self.programs.insert(0, zero);
        }
    }

    pub fn generate(&mut self) -> Option<u32> {
        let name = self.next.checked_add(1)?;
        self.next = name;
        self.programs.insert(name, None);
        Some(name)
    }

    pub fn delete(&mut self, name: u32) {
        self.programs.remove(&name);
        if self.bound == name {
            self.bound = 0;
            self.retained = None;
        }
    }

    pub fn bind(&mut self, name: u32) -> bool {
        if name != 0 && !self.programs.contains_key(&name) {
            return false;
        }
        self.bound = name;
        self.retained = self.programs.get(&name).cloned().flatten();
        true
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn load_bound(
        &mut self,
        program: BasicProgram,
        texture_units: usize,
    ) -> Result<(), UnsupportedOperation> {
        if !program.supports_texture_units(texture_units) {
            return Err(UnsupportedOperation);
        }
        let program = std::sync::Arc::new(program);
        self.retained = Some(program.clone());
        self.programs.insert(self.bound, Some(program));
        Ok(())
    }

    pub fn set_constant(&mut self, index: usize, value: [i32; 4]) -> bool {
        let Some(constant) = self.constants.get_mut(index) else {
            return false;
        };
        *constant = value.map(|v| {
            let magnitude = (v.unsigned_abs() >> 8).min(511) as f32 / 256.0;
            if v < 0 {
                -magnitude
            } else {
                magnitude
            }
        });
        true
    }

    pub fn constants(&self) -> [[f32; 4]; 4] {
        self.constants
    }

    pub fn active(&self) -> Option<&BasicProgram> {
        self.active_shared().map(std::sync::Arc::as_ref)
    }

    pub(crate) fn active_shared(&self) -> Option<&std::sync::Arc<BasicProgram>> {
        if self.enabled {
            self.programs
                .get(&self.bound)
                .map_or(self.retained.as_ref(), |p| p.as_ref())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_color_can_be_mixed_with_a_constant_using_texture_weight() {
        let weight = Stage {
            alpha_output: 0x0670,
            alpha_inputs: 0xc104_0000,
            flags: 0xc8,
            color_output: 0x8070,
            color_input_modifiers: 0x0400,
            color_inputs: 0xd508_0000,
            color_input_modes: 0x8100,
            texture_routing: 0x3fff_fc08,
        };
        let mix = Stage {
            alpha_output: 0x0670,
            alpha_inputs: 0xc324_4300,
            flags: 0xef,
            color_output: 0x8070,
            color_input_modifiers: 0x0900,
            color_inputs: 0xe400_4300,
            color_input_modes: 0x8888,
            texture_routing: 0x7fff_ffff,
        };
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![weight, mix],
        }
        .compile_basic()
        .unwrap();
        let duplicate = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![weight, mix],
        }
        .compile_basic()
        .unwrap();
        assert_ne!(program.id(), duplicate.id());
        assert_eq!(
            program.diagnostic_fingerprint(),
            duplicate.diagnostic_fingerprint()
        );
        let color = program.evaluate_sampled(
            [1.; 4],
            [[0.; 4], [0.; 4], [0.; 4], [0.75; 4]],
            [0.25, 0.5, 0.75, 1.],
            |_| [0., 0.25, 0., 1.],
        );
        assert_eq!(color, [0.375, 0.5625, 0.75, 0.9375]);
    }

    #[test]
    fn ar11_signed_alpha_maximum_routes_both_outputs() {
        let stage = Stage {
            alpha_output: 0x1121,
            alpha_inputs: 0xf804_dc04,
            flags: 0xca,
            color_output: 0,
            color_input_modifiers: 0,
            color_inputs: 0,
            color_input_modes: 0,
            texture_routing: 0x3fff_fc08,
        };
        let compile = |flags| {
            Program {
                architecture: Architecture::Ar11,
                flags,
                stages: vec![stage],
            }
            .compile_basic()
            .unwrap()
        };
        let maximum = compile(0);
        let positive = compile(1);
        let evaluate = |program: &BasicProgram, red| {
            program.evaluate_sampled([0.; 4], [[0.; 4]; 4], [0.; 4], |_| [red, 0., 0., 1.])[3]
        };
        for (red, magnitude, positive_part) in [(0.25, 0.5, 0.), (0.5, 0., 0.), (0.75, 0.5, 0.5)] {
            assert_eq!(evaluate(&maximum, red), magnitude);
            assert_eq!(evaluate(&positive, red), positive_part);
        }
    }

    #[test]
    fn paired_multiply_writes_two_registers() {
        let first = Stage {
            alpha_output: 0,
            alpha_inputs: 0,
            flags: 0x20,
            color_output: 0x4010,
            color_input_modifiers: 0x2414,
            color_inputs: 0x8408_4208,
            color_input_modes: 0x0101,
            texture_routing: 0xffff_ffff,
        };
        let second = Stage {
            color_output: 0x8070,
            color_input_modifiers: 0x0404,
            color_inputs: 0x0008_0108,
            color_input_modes: 0x8181,
            ..first
        };
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![first, second],
        }
        .compile_basic()
        .unwrap();
        assert_eq!(
            program.evaluate_sampled([1.; 4], [[0.; 4]; 4], [0.; 4], |_| unreachable!()),
            [0.75, 0.75, 0.75, 0.],
        );
        let alternate = Program {
            architecture: Architecture::Ar11,
            flags: 1,
            stages: vec![first, second],
        }
        .compile_basic()
        .unwrap();
        assert_eq!(
            alternate.evaluate_sampled([1.; 4], [[0.; 4]; 4], [0.; 4], |_| unreachable!()),
            [0.25, 0.25, 0.25, 0.],
        );
    }

    #[test]
    fn framebuffer_fetch_and_texture_alpha_feed_the_same_stage() {
        let stage = Stage {
            alpha_output: 0x8670,
            alpha_inputs: 0x0204_0000,
            flags: 0xe0,
            color_output: 0x8070,
            color_input_modifiers: 0x0400,
            color_inputs: 0xff08_0000,
            color_input_modes: 0x8100,
            texture_routing: 0x7fff_ffff,
        };
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![
                stage,
                Stage {
                    alpha_output: 0x0670,
                    alpha_inputs: 0xc304_0000,
                    flags: 0xe8,
                    color_output: 0x8070,
                    color_inputs: 0xe408_0000,
                    texture_routing: 0xffff_ffff,
                    ..stage
                },
            ],
        }
        .compile_basic()
        .unwrap();
        assert!(program.reads_framebuffer());
        assert!(program.routes().is_empty());
        assert_eq!(
            program.evaluate_sampled(
                [1.; 4],
                [[0.; 4]; 4],
                [0.1, 0.2, 0.3, 0.9],
                |_| unreachable!()
            ),
            [0.1, 0.2, 0.3, 0.5],
        );

        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![stage],
        }
        .compile_basic()
        .unwrap();
        assert_eq!(
            program.evaluate_sampled(
                [1.; 4],
                [[0.; 4]; 4],
                [0.1, 0.2, 0.3, 0.9],
                |_| unreachable!()
            ),
            [0.5, 0.5, 0.5, 0.],
        );
    }

    #[test]
    fn decodes_unaligned_little_endian_stage_fields() {
        let mut bytes = b"AR11\x81".to_vec();
        bytes.extend(1..=21);
        let program = Program::decode(&bytes).unwrap();
        assert_eq!(program.architecture, Architecture::Ar11);
        assert_eq!(program.flags, 2);
        assert_eq!(program.stages[0].alpha_output, 0x0201);
        assert_eq!(program.stages[0].alpha_inputs, 0x0605_0403);
        assert_eq!(program.stages[0].color_output, 0x0908);
        assert_eq!(program.stages[0].color_inputs, 0x0f0e_0d0c);
        assert_eq!(program.stages[0].texture_routing, 0x1514_1312);
    }

    fn alpha_predicate(not_equal: bool) -> BasicProgram {
        Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![Stage {
                alpha_output: 0x70 | u16::from(not_equal),
                // Primary alpha times one, compared with zero times one.
                alpha_inputs: 0x8304_0004,
                flags: 0x68,
                color_output: 0,
                color_input_modifiers: 0,
                color_inputs: 0,
                color_input_modes: 0,
                texture_routing: 0xffff_ffff,
            }],
        }
        .compile_basic()
        .unwrap()
    }

    #[test]
    fn ar11_alpha_predicates_discard_instead_of_writing_zero() {
        for not_equal in [false, true] {
            let program = alpha_predicate(not_equal);
            for alpha in [0., 0.25, 1.] {
                let output = program.evaluate_fragment(
                    [1., 1., 1., alpha],
                    [[0.; 4]; 4],
                    [0.; 4],
                    |_, _| unreachable!(),
                    |_| unreachable!(),
                );
                assert_eq!(output.is_none(), (alpha != 0.) == not_equal);
            }
            assert!(program.fragment_function().contains("discard"));
        }
    }

    #[test]
    fn shared_program_names_keep_bindings_and_constants_context_local() {
        let mut group = ProgramObjects::default();
        let mut first = ProgramObjects::default();
        let mut second = ProgramObjects::default();
        let name = first.generate().unwrap();
        assert!(first.bind(name));
        first.load_bound(alpha_predicate(false), 4).unwrap();
        let id = first.programs[&name].as_ref().unwrap().id();
        first.set_enabled(true);
        first.set_constant(0, [65536; 4]);
        first.exchange_namespace(&mut group);
        second.exchange_namespace(&mut group);
        assert!(second.active().is_none());
        assert_eq!(second.constants(), [[0.; 4]; 4]);
        assert!(second.bind(name));
        second.set_enabled(true);
        assert_eq!(second.active().unwrap().id(), id);
        assert!(second.generate().unwrap() > name);
        second.delete(name);
        second.exchange_namespace(&mut group);
        first.exchange_namespace(&mut group);
        // A deleted name cannot be rebound, but an existing binding survives.
        assert_eq!(first.active().unwrap().id(), id);
        assert_eq!(first.constants()[0], [1.; 4]);
        assert!(!first.bind(name));
        assert!(first.bind(0));
        assert!(first.active().is_none());
        let mut independent = ProgramObjects::default();
        assert!(!independent.bind(name));
    }

    #[test]
    fn ar11_coordinate_capacity_does_not_expand_the_ar10_encoding() {
        let stage = |coordinate: u32| Stage {
            alpha_output: 0,
            alpha_inputs: 0,
            flags: 0,
            color_output: 0,
            color_input_modifiers: 0,
            color_inputs: 0,
            color_input_modes: 0,
            texture_routing: 0x3fff_fc00 | coordinate << 5 | (coordinate + 8),
        };
        for (architecture, coordinate, accepted) in [
            (Architecture::Ar10, 3, true),
            (Architecture::Ar10, 4, false),
            (Architecture::Ar11, 5, true),
            (Architecture::Ar11, 7, true),
        ] {
            let result = Program {
                architecture,
                flags: 0,
                stages: vec![stage(coordinate)],
            }
            .compile_basic();
            assert_eq!(result.is_ok(), accepted);
            if coordinate == 7 {
                assert!(!result.unwrap().supports_texture_units(6));
            }
        }
        assert!(compile_term(Operand::register(2, [0; 3], [0; 3]), 0, 0, false).is_err());
    }

    #[test]
    fn ar11_color_interpolation_and_coissued_texture_alpha_need_no_fetch() {
        let stage = Stage {
            alpha_output: 0x8070,
            alpha_inputs: 0x9e41_9d40,
            flags: 0xef,
            color_output: 0x9070,
            color_input_modifiers: 0x7004,
            color_inputs: 0x8040_ff08,
            color_input_modes: 0x8881,
            // Color reads v1.x, v1.y and v2.x. Alpha's selector is unused.
            texture_routing: 0xc291_4bff,
        };
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![stage],
        }
        .compile_basic()
        .unwrap();
        assert!(program.requires_sc15());
        assert_eq!(program.coordinate_units(), 0b110);
        assert_eq!(program.texture_units(), 0);
        assert!(!program.supports_texture_units(2));
        assert!(program.supports_texture_units(3));
        let shader = program.fragment_function();
        assert!(shader.contains("uv1.x") && shader.contains("uv2.x"));
        assert!(shader.find("texel.a=").unwrap() < shader.find("(texel.a*").unwrap());
        let evaluate = |x| {
            program.evaluate_fragment(
                [1.; 4],
                [[0.5; 4]; 4],
                [0.; 4],
                |unit, component| {
                    if unit == 1 && component == 0 {
                        x
                    } else {
                        0.625
                    }
                },
                |_| unreachable!(),
            )
        };
        assert_eq!(evaluate(0.25), Some([0.; 4]));
        assert_eq!(evaluate(0.75), Some([0.5, 0.5, 0.5, 0.]));
    }

    #[test]
    fn ar11_signed_quarter_literals_cover_the_full_encoding() {
        for code in 0..32 {
            let term = compile_term(Operand::literal([code; 3]), 0, 0, true).unwrap();
            let expected = (code & 15) as f32 * 0.25 * if code & 16 == 0 { 1. } else { -1. };
            assert!(matches!(term.source, Source::Literal(value) if value == expected));
        }
        assert!(compile_term(Operand::literal([32; 3]), 0, 0, true).is_err());
        assert!(compile_term(Operand::literal([5; 3]), 0, 0, false).is_err());
    }

    #[test]
    fn environment_constants_use_signed_ten_bit_precision() {
        let mut objects = ProgramObjects::default();
        assert!(objects.set_constant(0, [255, 256, -257, 131072]));
        assert_eq!(
            objects.constants()[0],
            [0.0, 1.0 / 256.0, -1.0 / 256.0, 511.0 / 256.0],
        );
        assert!(!objects.set_constant(4, [0; 4]));
    }

    #[test]
    fn rejects_unknown_formats_truncated_records_and_invalid_counts() {
        assert_eq!(
            Program::decode(b"AR12\x01"),
            Err(DecodeError::UnknownArchitecture)
        );
        assert_eq!(Program::decode(b"AR11\x01"), Err(DecodeError::Truncated));
        assert_eq!(
            Program::decode(b"AR10\x05"),
            Err(DecodeError::InvalidStageCount)
        );
        assert_eq!(
            Program::decode(b"AR11\x00"),
            Err(DecodeError::InvalidStageCount)
        );
        assert_eq!(
            Program::decode(b"AR11\x0d"),
            Err(DecodeError::InvalidStageCount)
        );
    }
}
