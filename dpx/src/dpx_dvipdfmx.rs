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
    mutable_transmutes,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_mut
)]

use super::dpx_dvi::{
    dvi_close, dvi_comment, dvi_do_page, dvi_init, dvi_npages, dvi_reset_global_state,
    dvi_scan_specials, dvi_set_verbose, ReadLength,
};
use super::dpx_pdfdev::{
    Rect, Coord,
    pdf_close_device, pdf_dev_reset_global_state, pdf_dev_set_verbose, pdf_init_device,
};
use super::dpx_pdfdoc::pdf_doc_set_mediabox;
use super::dpx_pdfdoc::{
    pdf_close_document, pdf_doc_set_creator, pdf_doc_set_verbose, pdf_open_document,
};
use super::dpx_pdffont::{
    pdf_font_reset_unique_tag_state, pdf_font_set_deterministic_unique_tags, pdf_font_set_dpi,
};
use super::dpx_tt_aux::tt_aux_set_verbose;
use crate::dpx_pdfparse::parse_unsigned;
use crate::DisplayExt;
use crate::info;
use std::ffi::CStr;

use super::dpx_cid::CIDFont_set_flags;
use super::dpx_dpxconf::{paperinfo, defaultpapername, systempapername};
use super::dpx_dpxfile::{dpx_delete_old_cache, dpx_file_set_verbose};
use super::dpx_error::shut_up;
use super::dpx_fontmap::{
    pdf_close_fontmaps, pdf_fontmap_set_verbose, pdf_init_fontmaps, pdf_load_fontmap_file,
};
use super::dpx_mem::{new, renew};
use super::dpx_pdfencrypt::{pdf_enc_compute_id_string, pdf_enc_set_passwd, pdf_enc_set_verbose};
use super::dpx_pdfobj::{
    pdf_files_close, pdf_files_init, pdf_get_version, pdf_obj_reset_global_state,
    pdf_obj_set_verbose, pdf_set_compression, pdf_set_use_predictor, pdf_set_version,
};
use super::dpx_tfm::tfm_reset_global_state;
use super::dpx_vf::vf_reset_global_state;
use crate::specials::{
    spc_exec_at_begin_document, spc_exec_at_end_document, tpic::tpic_set_fill_mode,
};
use libc::{atoi, free, strlen};
use std::slice::from_raw_parts;

pub type PageRange = page_range;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct page_range {
    pub first: i32,
    pub last: i32,
}
#[no_mangle]
pub static mut is_xdv: i32 = 0i32;
#[no_mangle]
pub static mut translate_origin: i32 = 0i32;
static mut ignore_colors: i8 = 0_i8;
static mut annot_grow: f64 = 0.0f64;
static mut bookmark_open: i32 = 0i32;
static mut mag: f64 = 1.0f64;
static mut font_dpi: i32 = 600i32;
/*
 * Precision is essentially limited to 0.01pt.
 * See, dev_set_string() in pdfdev.c.
 */
static mut pdfdecimaldigits: i32 = 3i32;
/* Image cache life in hours */
/*  0 means erase all old images and leave new images */
/* -1 means erase all old images and also erase new images */
/* -2 means ignore image cache (default) */
static mut image_cache_life: i32 = -2i32;
/* Encryption */
static mut do_encryption: i32 = 0i32;
static mut key_bits: i32 = 40i32;
static mut permission: i32 = 0x3ci32;
/* Page device */
#[no_mangle]
pub static mut paper_width: f64 = 595.0f64;
#[no_mangle]
pub static mut paper_height: f64 = 842.0f64;
static mut x_offset: f64 = 72.0f64;
static mut y_offset: f64 = 72.0f64;
#[no_mangle]
pub static mut landscape_mode: i32 = 0i32;
#[no_mangle]
pub static mut always_embed: i32 = 0i32;
unsafe fn select_paper(paperspec: &[u8]) {
    let mut error: i32 = 0i32;
    paper_width = 0.;
    paper_height = 0.;
    if let Some(pi) = paperinfo(paperspec) {
        paper_width = (*pi).pswidth;
        paper_height = (*pi).psheight;
    } else {
        let comma = paperspec.iter().position(|&x| x == b',')
            .expect(&format!("Unrecognized paper format: {}", paperspec.display()));
        if let (Ok(width), Ok(height)) = ((&paperspec[..comma]).read_length_no_mag(), (&paperspec[comma+1..]).read_length_no_mag()) {
            paper_width = width;
            paper_height = height;
        } else {
            error = -1;
        }
    }
    if error != 0 || paper_width <= 0. || paper_height <= 0. {
        panic!(
            "Invalid paper size: {} ({:.2}x{:.2}",
            paperspec.display(),
            paper_width,
            paper_height,
        );
    };
}
unsafe fn select_pages(
    mut pagespec: *const i8,
    mut ret_page_ranges: *mut *mut PageRange,
    mut ret_num_page_ranges: *mut u32,
) {
    let mut page_ranges: *mut PageRange = 0 as *mut PageRange;
    let mut num_page_ranges: u32 = 0_u32;
    let mut max_page_ranges: u32 = 0_u32;
    let mut p: *const i8 = pagespec;
    while *p as i32 != '\u{0}' as i32 {
        /* Enlarge page range table if necessary */
        if num_page_ranges >= max_page_ranges {
            max_page_ranges = max_page_ranges.wrapping_add(4_u32); /* Can't be signed. */
            page_ranges = renew(
                page_ranges as *mut libc::c_void,
                (max_page_ranges as u64).wrapping_mul(::std::mem::size_of::<PageRange>() as u64)
                    as u32,
            ) as *mut PageRange
        }
        (*page_ranges.offset(num_page_ranges as isize)).first = 0i32;
        (*page_ranges.offset(num_page_ranges as isize)).last = 0i32;
        while *p as i32 != 0 && libc::isspace(*p as _) != 0 {
            p = p.offset(1)
        }
        let q = parse_unsigned(&mut p, p.offset(strlen(p) as isize));
        if !q.is_null() {
            /* '-' is allowed here */
            (*page_ranges.offset(num_page_ranges as isize)).first = atoi(q) - 1i32; /* Root node */
            (*page_ranges.offset(num_page_ranges as isize)).last =
                (*page_ranges.offset(num_page_ranges as isize)).first;
            free(q as *mut libc::c_void);
        }
        while *p as i32 != 0 && libc::isspace(*p as _) != 0 {
            p = p.offset(1)
        }
        if *p as i32 == '-' as i32 {
            p = p.offset(1);
            while *p as i32 != 0 && libc::isspace(*p as _) != 0 {
                p = p.offset(1)
            }
            (*page_ranges.offset(num_page_ranges as isize)).last = -1i32;
            if *p != 0 {
                let q = parse_unsigned(&mut p, p.offset(strlen(p) as isize));
                if !q.is_null() {
                    (*page_ranges.offset(num_page_ranges as isize)).last = atoi(q) - 1i32;
                    free(q as *mut libc::c_void);
                }
                while *p as i32 != 0 && libc::isspace(*p as _) != 0 {
                    p = p.offset(1)
                }
            }
        } else {
            (*page_ranges.offset(num_page_ranges as isize)).last =
                (*page_ranges.offset(num_page_ranges as isize)).first
        }
        num_page_ranges = num_page_ranges.wrapping_add(1);
        if *p as i32 == ',' as i32 {
            p = p.offset(1)
        } else {
            while *p as i32 != 0 && libc::isspace(*p as _) != 0 {
                p = p.offset(1)
            }
            if *p != 0 {
                panic!(
                    "Bad page range specification: {}",
                    CStr::from_ptr(p).display()
                );
            }
        }
    }
    *ret_page_ranges = page_ranges;
    *ret_num_page_ranges = num_page_ranges;
}
unsafe fn system_default() {
    if !systempapername().is_empty() {
        select_paper(systempapername());
    } else if !defaultpapername().is_empty() {
        select_paper(defaultpapername());
    };
}
unsafe fn do_dvi_pages(mut page_ranges: *mut PageRange, mut num_page_ranges: u32) {
    let mut mediabox = Rect::zero();
    spc_exec_at_begin_document();
    let mut page_width = paper_width;
    let init_paper_width = page_width;
    let mut page_height = paper_height;
    let init_paper_height = page_height;
    let mut page_count = 0;
    mediabox.ll = Coord::zero();
    mediabox.ur = Coord::new(paper_width, paper_height);
    pdf_doc_set_mediabox(0_u32, &mediabox);
    let mut i = 0;
    while i < num_page_ranges && dvi_npages() != 0 {
        if (*page_ranges.offset(i as isize)).last < 0i32 {
            let ref mut fresh0 = (*page_ranges.offset(i as isize)).last;
            *fresh0 = (*fresh0 as u32).wrapping_add(dvi_npages()) as i32 as i32
        }
        let step = if (*page_ranges.offset(i as isize)).first <= (*page_ranges.offset(i as isize)).last
        {
            1i32
        } else {
            -1i32
        };
        let mut page_no = (*page_ranges.offset(i as isize)).first;
        while dvi_npages() != 0 {
            if (page_no as u32) < dvi_npages() {
                info!("[{}", page_no + 1);
                /* Users want to change page size even after page is started! */
                page_width = paper_width;
                page_height = paper_height;
                let mut w = page_width;
                let mut h = page_height;
                let mut lm = landscape_mode;
                let mut xo = x_offset;
                let mut yo = y_offset;
                dvi_scan_specials(
                    page_no,
                    &mut w,
                    &mut h,
                    &mut xo,
                    &mut yo,
                    &mut lm,
                    0 as *mut i32,
                    0 as *mut i32,
                    0 as *mut i32,
                    0 as *mut i32,
                    0 as *mut i32,
                    0 as *mut i8,
                    0 as *mut i8,
                );
                if lm != landscape_mode {
                    let mut _tmp: f64 = w;
                    w = h;
                    h = _tmp;
                    landscape_mode = lm
                }
                if page_width != w || page_height != h {
                    page_width = w;
                    page_height = h
                }
                if x_offset != xo || y_offset != yo {
                    x_offset = xo;
                    y_offset = yo
                }
                if page_width != init_paper_width || page_height != init_paper_height {
                    mediabox = Rect::new((0., 0.), (page_width, page_height));
                    pdf_doc_set_mediabox(page_count + 1, &mediabox);
                }
                dvi_do_page(page_height, x_offset, y_offset);
                page_count = page_count + 1;
                info!("]");
            }
            if step > 0i32 && page_no >= (*page_ranges.offset(i as isize)).last {
                break;
            }
            if step < 0i32 && page_no <= (*page_ranges.offset(i as isize)).last {
                break;
            }
            page_no += step
        }
        i = i.wrapping_add(1)
    }
    if page_count < 1_u32 {
        panic!("No pages fall in range!");
    }
    spc_exec_at_end_document();
}
#[no_mangle]
pub unsafe extern "C" fn dvipdfmx_main(
    mut pdf_filename: *const i8,
    mut dvi_filename: *const i8,
    mut pagespec: *const i8,
    mut opt_flags: i32,
    mut translate: bool,
    mut compress: bool,
    mut deterministic_tags: bool,
    mut quiet: bool,
    mut verbose: u32,
) -> i32 {
    let mut enable_object_stream: bool = true; /* This must come before parsing options... */
    let mut num_page_ranges: u32 = 0_u32;
    let mut page_ranges: *mut PageRange = 0 as *mut PageRange;
    assert!(!pdf_filename.is_null());
    assert!(!dvi_filename.is_null());
    translate_origin = translate as i32;
    dvi_reset_global_state();
    tfm_reset_global_state();
    vf_reset_global_state();
    pdf_dev_reset_global_state();
    pdf_obj_reset_global_state();
    pdf_font_reset_unique_tag_state();
    if quiet {
        shut_up(2i32);
    } else {
        dvi_set_verbose(verbose as i32);
        pdf_dev_set_verbose(verbose as i32);
        pdf_doc_set_verbose(verbose as i32);
        pdf_enc_set_verbose(verbose as i32);
        pdf_obj_set_verbose(verbose as i32);
        pdf_fontmap_set_verbose(verbose as i32);
        dpx_file_set_verbose(verbose as i32);
        tt_aux_set_verbose(verbose as i32);
    }
    pdf_set_compression(if compress as i32 != 0 { 9i32 } else { 0i32 });
    pdf_font_set_deterministic_unique_tags(if deterministic_tags as i32 != 0 {
        1i32
    } else {
        0i32
    });
    system_default();
    pdf_init_fontmaps();
    /* We used to read the config file here. It synthesized command-line
     * arguments, so we emulate the default TeXLive config file by copying those
     * code bits. */
    pdf_set_version(5_u32); /* last page */
    select_paper(b"letter");
    annot_grow = 0i32 as f64;
    bookmark_open = 0i32;
    key_bits = 40i32;
    permission = 0x3ci32;
    font_dpi = 600i32;
    pdfdecimaldigits = 5i32;
    image_cache_life = -2i32;
    pdf_load_fontmap_file(CStr::from_bytes_with_nul(b"pdftex.map\x00").unwrap(), '+' as i32);
    pdf_load_fontmap_file(CStr::from_bytes_with_nul(b"kanjix.map\x00").unwrap(), '+' as i32);
    pdf_load_fontmap_file(CStr::from_bytes_with_nul(b"ckx.map\x00").unwrap(), '+' as i32);
    if !pagespec.is_null() {
        select_pages(pagespec, &mut page_ranges, &mut num_page_ranges);
    }
    if page_ranges.is_null() {
        page_ranges = new((1_u64).wrapping_mul(::std::mem::size_of::<PageRange>() as u64) as u32)
            as *mut PageRange
    }
    if num_page_ranges == 0_u32 {
        (*page_ranges.offset(0)).first = 0i32;
        (*page_ranges.offset(0)).last = -1i32;
        num_page_ranges = 1_u32
    }
    /*kpse_init_prog("", font_dpi, NULL, NULL);
    kpse_set_program_enabled(kpse_pk_format, true, kpse_src_texmf_cnf);*/
    pdf_font_set_dpi(font_dpi);
    dpx_delete_old_cache(image_cache_life);
    pdf_enc_compute_id_string(
        if dvi_filename.is_null() {
            None
        } else {
            Some(from_raw_parts(
                dvi_filename as *const u8,
                strlen(dvi_filename),
            ))
        },
        if pdf_filename.is_null() {
            None
        } else {
            Some(from_raw_parts(
                pdf_filename as *const u8,
                strlen(pdf_filename),
            ))
        },
    );
    let mut ver_major: i32 = 0i32;
    let mut ver_minor: i32 = 0i32;
    let mut owner_pw: [i8; 127] = [0; 127];
    let mut user_pw: [i8; 127] = [0; 127];
    /* Dependency between DVI and PDF side is rather complicated... */
    let dvi2pts = dvi_init(dvi_filename, mag);
    if dvi2pts == 0.0f64 {
        panic!("dvi_init() failed!");
    }
    pdf_doc_set_creator(dvi_comment());
    dvi_scan_specials(
        0i32,
        &mut paper_width,
        &mut paper_height,
        &mut x_offset,
        &mut y_offset,
        &mut landscape_mode,
        &mut ver_major,
        &mut ver_minor,
        &mut do_encryption,
        &mut key_bits,
        &mut permission,
        owner_pw.as_mut_ptr(),
        user_pw.as_mut_ptr(),
    );
    if ver_minor >= 3i32 && ver_minor <= 7i32 {
        pdf_set_version(ver_minor as u32);
    }
    if do_encryption != 0 {
        if !(key_bits >= 40i32 && key_bits <= 128i32 && key_bits % 8i32 == 0i32)
            && key_bits != 256i32
        {
            panic!("Invalid encryption key length specified: {}", key_bits);
        } else {
            if key_bits > 40i32 && pdf_get_version() < 4_u32 {
                panic!("Chosen key length requires at least PDF 1.4. Use \"-V 4\" to change.");
            }
        }
        do_encryption = 1i32;
        pdf_enc_set_passwd(
            key_bits as u32,
            permission as u32,
            owner_pw.as_mut_ptr(),
            user_pw.as_mut_ptr(),
        );
    }
    if landscape_mode != 0 {
        let mut _tmp: f64 = paper_width;
        paper_width = paper_height;
        paper_height = _tmp
    }
    pdf_files_init();
    if opt_flags & 1i32 << 6i32 != 0 {
        enable_object_stream = false
    }
    /* Set default paper size here so that all page's can inherite it.
     * annot_grow:    Margin of annotation.
     * bookmark_open: Miximal depth of open bookmarks.
     */
    pdf_open_document(
        pdf_filename,
        do_encryption != 0,
        enable_object_stream,
        paper_width,
        paper_height,
        annot_grow,
        bookmark_open,
        (opt_flags & 1i32 << 4i32 == 0) as i32,
    );
    /* Ignore_colors placed here since
     * they are considered as device's capacity.
     */
    pdf_init_device(dvi2pts, pdfdecimaldigits, ignore_colors as i32);
    if opt_flags & 1i32 << 2i32 != 0 {
        CIDFont_set_flags(1i32 << 1i32);
    }
    /* Please move this to spc_init_specials(). */
    if opt_flags & 1i32 << 1i32 != 0 {
        tpic_set_fill_mode(1i32); /* No prediction */
    }
    if opt_flags & 1i32 << 5i32 != 0 {
        pdf_set_use_predictor(0i32);
    }
    do_dvi_pages(page_ranges, num_page_ranges);
    pdf_files_close();
    /* Order of close... */
    pdf_close_device();
    /* pdf_close_document flushes XObject (image) and other resources. */
    pdf_close_document(); /* pdf_font may depend on fontmap. */
    pdf_close_fontmaps();
    dvi_close();
    info!("\n");
    free(page_ranges as *mut libc::c_void);
    0i32
}
