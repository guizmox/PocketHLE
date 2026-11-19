//! Native-driver regression for relocated guest uniform identities.
use super::super::*;
use pocket_cpu::regs::ArmReg::{R0, R1, R2, R3};
use pocket_kernel::Thunk;
use pocket_pe::ImportBinding;

pub(in crate::gles2::renderer) fn call(
    renderer: &mut Renderer,
    ctx: &mut CallCtx<'_>,
    name: &str,
    args: &[u32],
) -> Result<u32> {
    for (reg, &value) in [R0, R1, R2, R3].into_iter().zip(args) {
        ctx.cpu.write_reg(reg, value)?;
    }
    for (index, value) in args.iter().skip(4).enumerate() {
        ctx.cpu
            .write_mem(0x4000 + index as u32 * 4, &value.to_le_bytes())?;
    }
    renderer.gl(name, ctx)
}

fn shader(renderer: &mut Renderer, ctx: &mut CallCtx<'_>, kind: u32, source: &str) -> Result<u32> {
    let shader = call(renderer, ctx, "glCreateShader", &[kind])?;
    ctx.cpu.write_mem(0x2000, source.as_bytes())?;
    ctx.cpu.write_mem(0x1000, &0x2000u32.to_le_bytes())?;
    ctx.cpu
        .write_mem(0x1004, &(source.len() as u32).to_le_bytes())?;
    call(
        renderer,
        ctx,
        "glShaderSource",
        &[shader, 1, 0x1000, 0x1004],
    )?;
    call(renderer, ctx, "glCompileShader", &[shader])?;
    call(renderer, ctx, "glGetShaderiv", &[shader, 0x8b81, 0x1010])?;
    assert_eq!(ctx.cpu.read_mem(0x1010, 4)?, 1u32.to_le_bytes());
    Ok(shader)
}

fn location(
    renderer: &mut Renderer,
    ctx: &mut CallCtx<'_>,
    program: u32,
    name: &str,
) -> Result<u32> {
    ctx.cpu.write_mem(0x2000, format!("{name}\0").as_bytes())?;
    call(renderer, ctx, "glGetUniformLocation", &[program, 0x2000])
}

fn read(
    renderer: &mut Renderer,
    ctx: &mut CallCtx<'_>,
    program: u32,
    location: u32,
    integer: bool,
    count: usize,
) -> Result<Vec<u32>> {
    ctx.cpu.write_mem(0x3000, &[0xa5; 68])?;
    call(
        renderer,
        ctx,
        if integer {
            "glGetUniformiv"
        } else {
            "glGetUniformfv"
        },
        &[program, location, 0x3000],
    )?;
    assert_eq!(ctx.cpu.read_mem(0x3000 + count as u32 * 4, 4)?, [0xa5; 4]);
    Ok(ctx
        .cpu
        .read_mem(0x3000, count as u32 * 4)?
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect())
}

fn roundtrip(renderer: &mut Renderer) -> Result<Renderer> {
    let saved = renderer.capture_context().map_err(error)?;
    let bytes = serde_json::to_vec(&saved).map_err(|err| error(err.to_string()))?;
    let saved: ContextSnapshot =
        serde_json::from_slice(&bytes).map_err(|err| error(err.to_string()))?;
    saved.restore(1).map_err(error)
}

#[test]
#[ignore = "requires native desktop OpenGL"]
fn relocated_uniforms_survive_typed_updates_repeated_capture_and_program_lifecycle() -> Result<()> {
    let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
    let thunk = Thunk {
        thunk_va: 0,
        iat_va: 0,
        dll: "libglesv2.dll".into(),
        binding: ImportBinding::Name("glUniform4f".into()),
        friendly_name: None,
    };
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let mut original = Renderer::new(4, 4, 1)?;
    assert!(!original.host.es);
    let program = call(&mut original, &mut ctx, "glCreateProgram", &[])?;
    let vertex = shader(
        &mut original,
        &mut ctx,
        0x8b31,
        "attribute vec2 p; void main(){gl_Position=vec4(p,0.,1.);}",
    )?;
    let fragment_source = "precision mediump float;
        uniform float f1; uniform vec2 f2; uniform vec3 f3; uniform vec4 f4;
        uniform int i1; uniform ivec2 i2; uniform ivec3 i3; uniform ivec4 i4;
        uniform mat2 m2; uniform mat3 m3; uniform mat4 m4;
        uniform vec4 colors[3]; uniform bool flag; uniform sampler2D tex;
        void main(){float v=f1+f2.x+f3.y+f4.z+float(i1+i2.x+i3.y+i4.z)
            +m2[0][0]+m3[1][1]+m4[2][2]+float(flag);
            gl_FragColor=colors[0]+colors[1]+colors[2]+vec4(v)
                +texture2D(tex,vec2(0.5))*0.001;}";
    let fragment = shader(&mut original, &mut ctx, 0x8b30, fragment_source)?;
    for item in [vertex, fragment] {
        call(&mut original, &mut ctx, "glAttachShader", &[program, item])?;
    }
    call(&mut original, &mut ctx, "glLinkProgram", &[program])?;
    call(&mut original, &mut ctx, "glUseProgram", &[program])?;
    let symbols = [
        "f1",
        "f2",
        "f3",
        "f4",
        "i1",
        "i2",
        "i3",
        "i4",
        "m2",
        "m3",
        "m4",
        "colors[0]",
        "colors[1]",
        "colors[2]",
        "flag",
        "tex",
    ];
    let mut locations = Vec::new();
    let mut pairs = Vec::new();
    // Simulate the saved guest namespace differing from this host. One
    // guest ID collides with another valid native uniform: invalid IDs
    // must not fall through to that native variable after restore.
    let native: Vec<_> = symbols
        .iter()
        .map(|symbol| location(&mut original, &mut ctx, program, symbol))
        .collect::<Result<_>>()?;
    assert!(native.iter().all(|&value| value != u32::MAX));
    for (index, &host) in native.iter().enumerate() {
        let guest = if index == 0 { native[1] } else { host + 100 };
        pairs.push((guest as i32, host as i32));
        locations.push(guest);
    }
    original
        .uniform_locations
        .insert(program, UniformLocations::restored(pairs)?.unwrap());
    original.uniform_program = program;
    for (symbol, &expected) in symbols.iter().zip(&locations) {
        assert_eq!(
            location(&mut original, &mut ctx, program, symbol)?,
            expected
        );
    }
    let mut renderer = roundtrip(&mut original)?;
    drop(original);
    for repeat in 0..2 {
        for (symbol, &expected) in symbols.iter().zip(&locations) {
            assert_eq!(
                location(&mut renderer, &mut ctx, program, symbol)?,
                expected
            );
        }
        assert_eq!(
            location(&mut renderer, &mut ctx, program, "colors")?,
            locations[11]
        );
        assert_eq!(
            location(&mut renderer, &mut ctx, program, "missing")?,
            u32::MAX
        );
        for integer in [false, true] {
            for components in 1..=4 {
                let slot = components - 1 + if integer { 4 } else { 0 };
                let values: Vec<_> = (1..=components)
                    .map(|i| {
                        if integer {
                            i as u32 + repeat
                        } else {
                            (i as f32 + repeat as f32).to_bits()
                        }
                    })
                    .collect();
                let mut args = vec![locations[slot]];
                args.extend(&values);
                call(
                    &mut renderer,
                    &mut ctx,
                    &format!("glUniform{components}{}", if integer { "i" } else { "f" }),
                    &args,
                )?;
                assert_eq!(
                    read(
                        &mut renderer,
                        &mut ctx,
                        program,
                        locations[slot],
                        integer,
                        components
                    )?,
                    values
                );
                let bytes: Vec<_> = values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                ctx.cpu.write_mem(0x2800, &bytes)?;
                call(
                    &mut renderer,
                    &mut ctx,
                    &format!("glUniform{components}{}v", if integer { "i" } else { "f" }),
                    &[locations[slot], 1, 0x2800],
                )?;
                assert_eq!(
                    read(
                        &mut renderer,
                        &mut ctx,
                        program,
                        locations[slot],
                        integer,
                        components
                    )?,
                    values
                );
            }
        }
        for dimension in 2..=4 {
            let values: Vec<_> = (1..=dimension * dimension)
                .map(|i| (i as f32).to_bits())
                .collect();
            let bytes: Vec<_> = values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            ctx.cpu.write_mem(0x2800, &bytes)?;
            call(
                &mut renderer,
                &mut ctx,
                &format!("glUniformMatrix{dimension}fv"),
                &[locations[dimension + 6], 1, 0, 0x2800],
            )?;
            assert_eq!(
                read(
                    &mut renderer,
                    &mut ctx,
                    program,
                    locations[dimension + 6],
                    false,
                    values.len()
                )?,
                values
            );
        }
        let colors: Vec<_> = (1..=12).map(|i| (i as f32 / 16.).to_bits()).collect();
        ctx.cpu.write_mem(
            0x2800,
            &colors
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )?;
        call(
            &mut renderer,
            &mut ctx,
            "glUniform4fv",
            &[locations[11], 3, 0x2800],
        )?;
        for i in 0..3 {
            assert_eq!(
                read(
                    &mut renderer,
                    &mut ctx,
                    program,
                    locations[11 + i],
                    false,
                    4
                )?,
                colors[i * 4..i * 4 + 4]
            );
        }
        call(&mut renderer, &mut ctx, "glUniform1i", &[locations[14], 1])?;
        call(&mut renderer, &mut ctx, "glUniform1i", &[locations[15], 2])?;
        assert_eq!(
            read(&mut renderer, &mut ctx, program, locations[14], true, 1)?,
            [1]
        );
        assert_eq!(
            read(&mut renderer, &mut ctx, program, locations[15], true, 1)?,
            [2]
        );
        // An invalid guest location that happens to be valid on this host
        // must raise INVALID_OPERATION without writing a different uniform.
        call(
            &mut renderer,
            &mut ctx,
            "glUniform1f",
            &[native[0], 42f32.to_bits()],
        )?;
        assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0x0502);
        call(
            &mut renderer,
            &mut ctx,
            "glGetUniformfv",
            &[program, native[0], 0x3000],
        )?;
        assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0x0502);
        call(
            &mut renderer,
            &mut ctx,
            "glUniform1f",
            &[u32::MAX, 42f32.to_bits()],
        )?;
        assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0);
        call(&mut renderer, &mut ctx, "glUseProgram", &[0xfffffff0])?;
        assert_ne!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0);
        call(&mut renderer, &mut ctx, "glUniform1i", &[locations[15], 1])?;
        assert_eq!(
            read(&mut renderer, &mut ctx, program, locations[15], true, 1)?,
            [1]
        );
        assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0);
        renderer = roundtrip(&mut renderer)?;
    }
    // Two native programs may use the same guest uniform ID independently.
    let other = call(&mut renderer, &mut ctx, "glCreateProgram", &[])?;
    let other_fragment = shader(
        &mut renderer,
        &mut ctx,
        0x8b30,
        "precision mediump float; uniform vec4 f4; void main(){gl_FragColor=f4;}",
    )?;
    for item in [vertex, other_fragment] {
        call(&mut renderer, &mut ctx, "glAttachShader", &[other, item])?;
    }
    call(&mut renderer, &mut ctx, "glLinkProgram", &[other])?;
    let other_host = location(&mut renderer, &mut ctx, other, "f4")?;
    let other_native_program = renderer.names.as_ref().unwrap().host(Kind::Program, other);
    renderer.uniform_locations.insert(
        other_native_program,
        UniformLocations::restored([(locations[3] as i32, other_host as i32)])?.unwrap(),
    );
    call(&mut renderer, &mut ctx, "glUseProgram", &[other])?;
    let blue = [0, 0, 1f32.to_bits(), 1f32.to_bits()];
    call(
        &mut renderer,
        &mut ctx,
        "glUniform4f",
        &[locations[3], blue[0], blue[1], blue[2], blue[3]],
    )?;
    assert_eq!(
        read(&mut renderer, &mut ctx, other, locations[3], false, 4)?,
        blue
    );
    assert_ne!(
        read(&mut renderer, &mut ctx, program, locations[3], false, 4)?,
        blue
    );
    // Exercise actual GPU drawing after relocation, not only uniform reads.
    call(&mut renderer, &mut ctx, "glVertexAttrib2f", &[0, 0, 0])?;
    call(&mut renderer, &mut ctx, "glDrawArrays", &[0, 0, 1])?;
    call(
        &mut renderer,
        &mut ctx,
        "glReadPixels",
        &[2, 2, 1, 1, RGBA, UNSIGNED_BYTE, 0x3000],
    )?;
    assert_eq!(ctx.cpu.read_mem(0x3000, 4)?, [0, 0, 255, 255]);
    // Failed relink keeps the previously installed executable and map;
    // successful relink resets values and native location identity.
    call(&mut renderer, &mut ctx, "glUseProgram", &[program])?;
    let duplicate = shader(
        &mut renderer,
        &mut ctx,
        0x8b31,
        "void main(){gl_Position=vec4(1.);}",
    )?;
    call(
        &mut renderer,
        &mut ctx,
        "glAttachShader",
        &[program, duplicate],
    )?;
    call(&mut renderer, &mut ctx, "glLinkProgram", &[program])?;
    call(
        &mut renderer,
        &mut ctx,
        "glGetProgramiv",
        &[program, 0x8b82, 0x1010],
    )?;
    assert_eq!(ctx.cpu.read_mem(0x1010, 4)?, [0; 4]);
    let host_program = renderer
        .names
        .as_ref()
        .unwrap()
        .host(Kind::Program, program);
    assert!(renderer.uniform_locations.contains_key(&host_program));
    call(&mut renderer, &mut ctx, "glUniform1i", &[locations[15], 0])?;
    // Preserve the real driver's failed-link error, even though the old
    // executable and its relocation table remain installed for drawing.
    assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0x0502);
    call(
        &mut renderer,
        &mut ctx,
        "glDetachShader",
        &[program, duplicate],
    )?;
    call(&mut renderer, &mut ctx, "glDeleteShader", &[duplicate])?;
    call(&mut renderer, &mut ctx, "glLinkProgram", &[program])?;
    assert!(!renderer.uniform_locations.contains_key(&host_program));
    let reset = location(&mut renderer, &mut ctx, program, "f4")?;
    assert_eq!(
        read(&mut renderer, &mut ctx, program, reset, false, 4)?,
        [0; 4]
    );
    call(&mut renderer, &mut ctx, "glDeleteProgram", &[other])?;
    assert!(!renderer
        .uniform_locations
        .contains_key(&other_native_program));
    // A delete-pending current program survives capture until UseProgram(0).
    let mut complete = Vec::new();
    for symbol in symbols {
        let native = location(&mut renderer, &mut ctx, program, symbol)? as i32;
        complete.push((native + 100, native));
    }
    renderer
        .uniform_locations
        .insert(host_program, UniformLocations::restored(complete)?.unwrap());
    call(&mut renderer, &mut ctx, "glDeleteProgram", &[program])?;
    renderer = roundtrip(&mut renderer)?;
    call(&mut renderer, &mut ctx, "glUseProgram", &[0])?;
    assert!(renderer.deleted_programs.is_empty());
    assert!(renderer.uniform_locations.is_empty());
    assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0);
    Ok(())
}

#[test]
#[ignore = "requires native OpenGL"]
pub(in crate::gles2::renderer) fn installed_executable_survives_failed_link_and_two_cold_snapshots(
) -> Result<()> {
    let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
    let thunk = Thunk {
        thunk_va: 0,
        iat_va: 0,
        dll: "libglesv2.dll".into(),
        binding: ImportBinding::Name("glDrawArrays".into()),
        friendly_name: None,
    };
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let mut renderer = Renderer::new(4, 4, 1)?;
    let vertex = shader(
        &mut renderer,
        &mut ctx,
        0x8b31,
        "void main(){gl_Position=vec4(0.,0.,0.,1.);gl_PointSize=1.;}",
    )?;
    let fragment = shader(
        &mut renderer,
        &mut ctx,
        0x8b30,
        "precision mediump float;uniform vec4 tint;void main(){gl_FragColor=tint;}",
    )?;
    let program = call(&mut renderer, &mut ctx, "glCreateProgram", &[])?;
    for shader in [vertex, fragment] {
        call(
            &mut renderer,
            &mut ctx,
            "glAttachShader",
            &[program, shader],
        )?;
    }
    call(&mut renderer, &mut ctx, "glLinkProgram", &[program])?;
    call(&mut renderer, &mut ctx, "glUseProgram", &[program])?;
    let tint = location(&mut renderer, &mut ctx, program, "tint")?;
    call(
        &mut renderer,
        &mut ctx,
        "glUniform4f",
        &[
            tint,
            0.25f32.to_bits(),
            0.5f32.to_bits(),
            0.75f32.to_bits(),
            1f32.to_bits(),
        ],
    )?;
    call(&mut renderer, &mut ctx, "glDrawArrays", &[0, 0, 1])?;
    call(
        &mut renderer,
        &mut ctx,
        "glReadPixels",
        &[0, 0, 4, 4, RGBA, UNSIGNED_BYTE, 0x3000],
    )?;
    let expected = ctx.cpu.read_mem(0x3000, 64)?;
    assert!(expected.chunks_exact(4).any(|pixel| pixel[1] != 0));
    // ES permits one attached shader of each type. Recompile that shader
    // unsuccessfully so LinkProgram genuinely fails on both native APIs.
    let invalid = "void main( invalid";
    ctx.cpu.write_mem(0x2000, invalid.as_bytes())?;
    ctx.cpu.write_mem(0x1000, &0x2000u32.to_le_bytes())?;
    ctx.cpu
        .write_mem(0x1004, &(invalid.len() as u32).to_le_bytes())?;
    call(
        &mut renderer,
        &mut ctx,
        "glShaderSource",
        &[vertex, 1, 0x1000, 0x1004],
    )?;
    call(&mut renderer, &mut ctx, "glCompileShader", &[vertex])?;
    call(&mut renderer, &mut ctx, "glLinkProgram", &[program])?;
    call(
        &mut renderer,
        &mut ctx,
        "glGetProgramiv",
        &[program, 0x8b82, 0x1010],
    )?;
    assert_eq!(ctx.cpu.read_mem(0x1010, 4)?, [0; 4]);
    for _ in 0..2 {
        renderer = roundtrip(&mut renderer)?;
        call(
            &mut renderer,
            &mut ctx,
            "glGetProgramiv",
            &[program, 0x8b82, 0x1010],
        )?;
        assert_eq!(ctx.cpu.read_mem(0x1010, 4)?, [0; 4]);
        call(&mut renderer, &mut ctx, "glClearColor", &[0, 0, 0, 0])?;
        call(&mut renderer, &mut ctx, "glClear", &[0x4000])?;
        call(&mut renderer, &mut ctx, "glDrawArrays", &[0, 0, 1])?;
        call(
            &mut renderer,
            &mut ctx,
            "glReadPixels",
            &[0, 0, 4, 4, RGBA, UNSIGNED_BYTE, 0x3000],
        )?;
        assert_eq!(ctx.cpu.read_mem(0x3000, 64)?, expected);
        assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0);
    }
    call(&mut renderer, &mut ctx, "glUseProgram", &[0])?;
    call(&mut renderer, &mut ctx, "glUseProgram", &[program])?;
    assert_eq!(call(&mut renderer, &mut ctx, "glGetError", &[])?, 0x0502);
    assert!(renderer.failed_program.is_none());
    Ok(())
}
