/* This is dvipdfmx, an eXtended version of dvipdfm by Mark A. Wicks.

    Copyright (C) 2002-2016 by Jin-Hwan Cho and Shunsaku Hirata,
    the dvipdfmx project team.

    Copyright (C) 1998, 1999 by Mark A. Wicks <mwicks@kettering.edu>

    This program is free software; you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation; either version 2 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program; if not, write to the Free Software
    Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA 02111-1307 USA.
*/
#![allow(
    dead_code,
    mutable_transmutes,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_assignments,
    unused_mut
)]

use super::spc_warn;
use super::util::{spc_util_read_colorspec, spc_util_read_numbers};
use crate::dpx_dpxutil::parse_c_ident;
use crate::dpx_fontmap::{
    is_pdfm_mapline, pdf_append_fontmap_record, pdf_clear_fontmap_record, pdf_init_fontmap_record,
    pdf_insert_fontmap_record, pdf_load_fontmap_file, pdf_read_fontmap_line,
    pdf_remove_fontmap_record,
};
use crate::dpx_mem::{new, xrealloc};
use crate::dpx_mfileio::work_buffer;
use crate::dpx_pdfdev::{pdf_dev_reset_color, pdf_dev_reset_fonts};
use crate::dpx_pdfdoc::{pdf_doc_add_page_content, pdf_doc_set_bgcolor};
use crate::dpx_pdfdraw::{
    pdf_dev_concat, pdf_dev_get_fixed_point, pdf_dev_grestore, pdf_dev_gsave,
    pdf_dev_set_fixed_point,
};
use crate::dpx_pdfparse::{parse_ident, parse_val_ident, skip_white};
use crate::streq_ptr;
use libc::{free, memcmp, sprintf, strlen, strncmp, strncpy};

pub type size_t = u64;

use super::{spc_arg, spc_env};

pub type spc_handler_fn_ptr = Option<unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32>;
use super::spc_handler;
use crate::dpx_fontmap::fontmap_rec;

use crate::dpx_pdfdev::pdf_coord;

use crate::dpx_pdfdev::pdf_tmatrix;

pub use crate::dpx_pdfcolor::pdf_color;

/* tectonic/core-strutils.h: miscellaneous C string utilities
   Copyright 2016-2018 the Tectonic Project
   Licensed under the MIT License.
*/
/* Note that we explicitly do *not* change this on Windows. For maximum
 * portability, we should probably accept *either* forward or backward slashes
 * as directory separators. */
#[no_mangle]
pub unsafe extern "C" fn spc_handler_xtx_do_transform(
    mut x_user: f64,
    mut y_user: f64,
    mut a: f64,
    mut b: f64,
    mut c: f64,
    mut d: f64,
    mut e: f64,
    mut f: f64,
) -> i32 {
    let mut M = pdf_tmatrix::new();
    /* Create transformation matrix */
    M.a = a;
    M.b = b;
    M.c = c;
    M.d = d;
    M.e = (1.0f64 - M.a) * x_user - M.c * y_user + e;
    M.f = (1.0f64 - M.d) * y_user - M.b * x_user + f;
    pdf_dev_concat(&mut M);
    let pt = pdf_dev_get_fixed_point();
    pdf_dev_set_fixed_point(x_user - pt.x, y_user - pt.y);
    0i32
}
unsafe extern "C" fn spc_handler_xtx_scale(mut spe: *mut spc_env, mut args: *mut spc_arg) -> i32 {
    let mut values: [f64; 2] = [0.; 2];
    if spc_util_read_numbers(&mut *values.as_mut_ptr().offset(0), 2i32, args) < 2i32 {
        return -1i32;
    }
    (*args).curptr = (*args).endptr;
    return spc_handler_xtx_do_transform(
        (*spe).x_user,
        (*spe).y_user,
        values[0],
        0i32 as f64,
        0i32 as f64,
        values[1],
        0i32 as f64,
        0i32 as f64,
    );
}
/* Scaling without gsave/grestore. */
static mut scaleFactors: *mut pdf_coord = 0 as *const pdf_coord as *mut pdf_coord;
static mut scaleFactorCount: i32 = -1i32;
unsafe extern "C" fn spc_handler_xtx_bscale(mut spe: *mut spc_env, mut args: *mut spc_arg) -> i32 {
    let mut values: [f64; 2] = [0.; 2];
    scaleFactorCount += 1;
    if scaleFactorCount & 0xfi32 == 0 {
        scaleFactors = xrealloc(
            scaleFactors as *mut libc::c_void,
            ((scaleFactorCount + 16i32) as u64)
                .wrapping_mul(::std::mem::size_of::<pdf_coord>() as u64),
        ) as *mut pdf_coord
    }
    if spc_util_read_numbers(&mut *values.as_mut_ptr().offset(0), 2i32, args) < 2i32 {
        return -1i32;
    }
    if values[0].abs() < 1.0e-7f64 || values[1].abs() < 1.0e-7f64 {
        return -1i32;
    }
    (*scaleFactors.offset(scaleFactorCount as isize)).x = 1i32 as f64 / values[0];
    (*scaleFactors.offset(scaleFactorCount as isize)).y = 1i32 as f64 / values[1];
    (*args).curptr = (*args).endptr;
    return spc_handler_xtx_do_transform(
        (*spe).x_user,
        (*spe).y_user,
        values[0],
        0i32 as f64,
        0i32 as f64,
        values[1],
        0i32 as f64,
        0i32 as f64,
    );
}
unsafe extern "C" fn spc_handler_xtx_escale(mut spe: *mut spc_env, mut args: *mut spc_arg) -> i32 {
    let fresh0 = scaleFactorCount;
    scaleFactorCount = scaleFactorCount - 1;
    let mut factor: pdf_coord = *scaleFactors.offset(fresh0 as isize);
    (*args).curptr = (*args).endptr;
    return spc_handler_xtx_do_transform(
        (*spe).x_user,
        (*spe).y_user,
        factor.x,
        0i32 as f64,
        0i32 as f64,
        factor.y,
        0i32 as f64,
        0i32 as f64,
    );
}
unsafe extern "C" fn spc_handler_xtx_rotate(mut spe: *mut spc_env, mut args: *mut spc_arg) -> i32 {
    let mut value: f64 = 0.;
    if spc_util_read_numbers(&mut value, 1i32, args) < 1i32 {
        return -1i32;
    }
    (*args).curptr = (*args).endptr;
    let (s, c) = (value * core::f64::consts::PI / 180.).sin_cos();
    spc_handler_xtx_do_transform(
        (*spe).x_user,
        (*spe).y_user,
        c,
        s,
        -s,
        c,
        0i32 as f64,
        0i32 as f64,
    )
}
#[no_mangle]
pub unsafe extern "C" fn spc_handler_xtx_gsave(
    mut _spe: *mut spc_env,
    mut _args: *mut spc_arg,
) -> i32 {
    pdf_dev_gsave();
    0i32
}
#[no_mangle]
pub unsafe extern "C" fn spc_handler_xtx_grestore(
    mut _spe: *mut spc_env,
    mut _args: *mut spc_arg,
) -> i32 {
    pdf_dev_grestore();
    /*
     * Unfortunately, the following line is necessary in case
     * of a font or color change inside of the save/restore pair.
     * Anything that was done there must be redone, so in effect,
     * we make no assumptions about what fonts. We act like we are
     * starting a new page.
     */
    pdf_dev_reset_fonts(0i32);
    pdf_dev_reset_color(0i32);
    0i32
}
/* Please remove this.
 * This should be handled before processing pages!
 */
unsafe extern "C" fn spc_handler_xtx_papersize(
    mut _spe: *mut spc_env,
    mut _args: *mut spc_arg,
) -> i32 {
    0i32
}
unsafe extern "C" fn spc_handler_xtx_backgroundcolor(
    mut spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    let mut error: i32 = 0;
    let mut colorspec = pdf_color {
        num_components: 0,
        spot_color_name: None,
        values: [0.; 4],
    };
    error = spc_util_read_colorspec(spe, &mut colorspec, args, 0i32);
    if error != 0 {
        spc_warn(
            spe,
            b"No valid color specified?\x00" as *const u8 as *const i8,
        );
    } else {
        pdf_doc_set_bgcolor(Some(&colorspec));
    }
    error
}
/* FIXME: xdv2pdf's x:fontmapline and x:fontmapfile may have slightly different syntax/semantics */
unsafe extern "C" fn spc_handler_xtx_fontmapline(
    mut spe: *mut spc_env,
    mut ap: *mut spc_arg,
) -> i32 {
    let mut mrec: *mut fontmap_rec = 0 as *mut fontmap_rec;
    let mut map_name: *mut i8 = 0 as *mut i8;
    let mut opchr: i8 = 0;
    let mut error: i32 = 0i32;
    static mut buffer: [i8; 1024] = [0; 1024];
    let mut p: *const i8 = 0 as *const i8;
    let mut q: *mut i8 = 0 as *mut i8;
    skip_white(&mut (*ap).curptr, (*ap).endptr);
    if (*ap).curptr >= (*ap).endptr {
        spc_warn(
            spe,
            b"Empty fontmapline special?\x00" as *const u8 as *const i8,
        );
        return -1i32;
    }
    opchr = *(*ap).curptr.offset(0);
    if opchr as i32 == '-' as i32 || opchr as i32 == '+' as i32 {
        (*ap).curptr = (*ap).curptr.offset(1)
    }
    skip_white(&mut (*ap).curptr, (*ap).endptr);
    match opchr as i32 {
        45 => {
            map_name = parse_ident(&mut (*ap).curptr, (*ap).endptr);
            if !map_name.is_null() {
                pdf_remove_fontmap_record(map_name);
                free(map_name as *mut libc::c_void);
            } else {
                spc_warn(
                    spe,
                    b"Invalid fontmap line: Missing TFM name.\x00" as *const u8 as *const i8,
                );
                error = -1i32
            }
        }
        _ => {
            p = (*ap).curptr;
            q = buffer.as_mut_ptr();
            while p < (*ap).endptr {
                let fresh1 = p;
                p = p.offset(1);
                let fresh2 = q;
                q = q.offset(1);
                *fresh2 = *fresh1
            }
            *q = '\u{0}' as i32 as i8;
            mrec = new((1_u64).wrapping_mul(::std::mem::size_of::<fontmap_rec>() as u64) as u32)
                as *mut fontmap_rec;
            pdf_init_fontmap_record(mrec);
            error = pdf_read_fontmap_line(
                mrec,
                buffer.as_mut_ptr(),
                (*ap).endptr.wrapping_offset_from((*ap).curptr) as i64 as i32,
                is_pdfm_mapline(buffer.as_mut_ptr()),
            );
            if error != 0 {
                spc_warn(spe, b"Invalid fontmap line.\x00" as *const u8 as *const i8);
            } else if opchr as i32 == '+' as i32 {
                pdf_append_fontmap_record((*mrec).map_name, mrec);
            } else {
                pdf_insert_fontmap_record((*mrec).map_name, mrec);
            }
            pdf_clear_fontmap_record(mrec);
            free(mrec as *mut libc::c_void);
        }
    }
    if error == 0 {
        (*ap).curptr = (*ap).endptr
    }
    0i32
}
unsafe extern "C" fn spc_handler_xtx_fontmapfile(
    mut spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    let mut mapfile: *mut i8 = 0 as *mut i8;
    let mut mode: i32 = 0;
    let mut error: i32 = 0i32;
    skip_white(&mut (*args).curptr, (*args).endptr);
    if (*args).curptr >= (*args).endptr {
        return 0i32;
    }
    match *(*args).curptr.offset(0) as i32 {
        45 => {
            mode = '-' as i32;
            (*args).curptr = (*args).curptr.offset(1)
        }
        43 => {
            mode = '+' as i32;
            (*args).curptr = (*args).curptr.offset(1)
        }
        _ => mode = 0i32,
    }
    mapfile = parse_val_ident(&mut (*args).curptr, (*args).endptr);
    if mapfile.is_null() {
        spc_warn(
            spe,
            b"No fontmap file specified.\x00" as *const u8 as *const i8,
        );
        return -1i32;
    } else {
        error = pdf_load_fontmap_file(mapfile, mode)
    }
    error
}
static mut overlay_name: [i8; 256] = [0; 256];
unsafe extern "C" fn spc_handler_xtx_initoverlay(
    mut _spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    skip_white(&mut (*args).curptr, (*args).endptr);
    if (*args).curptr >= (*args).endptr {
        return -1i32;
    }
    strncpy(
        overlay_name.as_mut_ptr(),
        (*args).curptr,
        (*args).endptr.wrapping_offset_from((*args).curptr) as _,
    );
    overlay_name[(*args).endptr.wrapping_offset_from((*args).curptr) as i64 as usize] = 0_i8;
    (*args).curptr = (*args).endptr;
    0i32
}
unsafe extern "C" fn spc_handler_xtx_clipoverlay(
    mut _spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    skip_white(&mut (*args).curptr, (*args).endptr);
    if (*args).curptr >= (*args).endptr {
        return -1i32;
    }
    pdf_dev_grestore();
    pdf_dev_gsave();
    if strncmp(
        overlay_name.as_mut_ptr(),
        (*args).curptr,
        strlen(overlay_name.as_mut_ptr()),
    ) != 0i32
        && strncmp(
            b"all\x00" as *const u8 as *const i8,
            (*args).curptr,
            strlen(b"all\x00" as *const u8 as *const i8),
        ) != 0i32
    {
        pdf_doc_add_page_content(b" 0 0 m W n\x00" as *const u8 as *const i8, 10_u32);
    }
    (*args).curptr = (*args).endptr;
    0i32
}
unsafe extern "C" fn spc_handler_xtx_renderingmode(
    mut spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    let mut value: f64 = 0.;
    if spc_util_read_numbers(&mut value, 1i32, args) < 1i32 {
        return -1i32;
    }
    if (value as i32) < 0i32 || value as i32 > 7i32 {
        spc_warn(
            spe,
            b"Invalid text rendering mode %d.\n\x00" as *const u8 as *const i8,
            value as i32,
        );
        return -1i32;
    }
    sprintf(
        work_buffer.as_mut_ptr(),
        b" %d Tr\x00" as *const u8 as *const i8,
        value as i32,
    );
    pdf_doc_add_page_content(
        work_buffer.as_mut_ptr(),
        strlen(work_buffer.as_mut_ptr()) as u32,
    );
    skip_white(&mut (*args).curptr, (*args).endptr);
    if (*args).curptr < (*args).endptr {
        pdf_doc_add_page_content(b" \x00" as *const u8 as *const i8, 1_u32);
        pdf_doc_add_page_content(
            (*args).curptr,
            (*args).endptr.wrapping_offset_from((*args).curptr) as i64 as u32,
        );
    }
    (*args).curptr = (*args).endptr;
    0i32
}
unsafe extern "C" fn spc_handler_xtx_unsupportedcolor(
    mut spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    spc_warn(spe,
             b"xetex-style \\special{x:%s} is not supported by this driver;\nupdate document or driver to use \\special{color} instead.\x00"
                 as *const u8 as *const i8, (*args).command);
    (*args).curptr = (*args).endptr;
    0i32
}
unsafe extern "C" fn spc_handler_xtx_unsupported(
    mut spe: *mut spc_env,
    mut args: *mut spc_arg,
) -> i32 {
    spc_warn(
        spe,
        b"xetex-style \\special{x:%s} is not supported by this driver.\x00" as *const u8
            as *const i8,
        (*args).command,
    );
    (*args).curptr = (*args).endptr;
    0i32
}
static mut xtx_handlers: [spc_handler; 21] = {
    [
        {
            let mut init = spc_handler {
                key: b"textcolor\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"textcolorpush\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"textcolorpop\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"rulecolor\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"rulecolorpush\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"rulecolorpop\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupportedcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"papersize\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_papersize
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"backgroundcolor\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_backgroundcolor
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"gsave\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_gsave
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"grestore\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_grestore
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"scale\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_scale
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"bscale\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_bscale
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"escale\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_escale
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"rotate\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_rotate
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"fontmapline\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_fontmapline
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"fontmapfile\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_fontmapfile
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"shadow\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupported
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"colorshadow\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_unsupported
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"renderingmode\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_renderingmode
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"initoverlay\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_initoverlay
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
        {
            let mut init = spc_handler {
                key: b"clipoverlay\x00" as *const u8 as *const i8,
                exec: Some(
                    spc_handler_xtx_clipoverlay
                        as unsafe extern "C" fn(_: *mut spc_env, _: *mut spc_arg) -> i32,
                ),
            };
            init
        },
    ]
};
#[no_mangle]
pub unsafe extern "C" fn spc_xtx_check_special(mut buf: *const i8, mut len: i32) -> bool {
    let mut p: *const i8 = 0 as *const i8;
    let mut endptr: *const i8 = 0 as *const i8;
    p = buf;
    endptr = p.offset(len as isize);
    skip_white(&mut p, endptr);
    if p.offset(strlen(b"x:\x00" as *const u8 as *const i8) as isize) <= endptr
        && memcmp(
            p as *const libc::c_void,
            b"x:\x00" as *const u8 as *const i8 as *const libc::c_void,
            strlen(b"x:\x00" as *const u8 as *const i8),
        ) == 0
    {
        return true;
    }
    false
}
#[no_mangle]
pub unsafe extern "C" fn spc_xtx_setup_handler(
    mut sph: *mut spc_handler,
    mut spe: *mut spc_env,
    mut ap: *mut spc_arg,
) -> i32 {
    let mut error: i32 = -1i32;
    let mut q: *mut i8 = 0 as *mut i8;
    assert!(!sph.is_null() && !spe.is_null() && !ap.is_null());
    skip_white(&mut (*ap).curptr, (*ap).endptr);
    if (*ap)
        .curptr
        .offset(strlen(b"x:\x00" as *const u8 as *const i8) as isize)
        >= (*ap).endptr
        || memcmp(
            (*ap).curptr as *const libc::c_void,
            b"x:\x00" as *const u8 as *const i8 as *const libc::c_void,
            strlen(b"x:\x00" as *const u8 as *const i8),
        ) != 0
    {
        spc_warn(spe, b"Not x: special???\x00" as *const u8 as *const i8);
        return -1i32;
    }
    (*ap).curptr = (*ap)
        .curptr
        .offset(strlen(b"x:\x00" as *const u8 as *const i8) as isize);
    skip_white(&mut (*ap).curptr, (*ap).endptr);
    q = parse_c_ident(&mut (*ap).curptr, (*ap).endptr);
    if !q.is_null() {
        for i in 0..(::std::mem::size_of::<[spc_handler; 21]>() as u64)
            .wrapping_div(::std::mem::size_of::<spc_handler>() as u64)
        {
            if streq_ptr(q, xtx_handlers[i as usize].key) {
                (*ap).command = xtx_handlers[i as usize].key;
                (*sph).key = b"x:\x00" as *const u8 as *const i8;
                (*sph).exec = xtx_handlers[i as usize].exec;
                skip_white(&mut (*ap).curptr, (*ap).endptr);
                error = 0i32;
                break;
            }
        }
        free(q as *mut libc::c_void);
    }
    error
}
