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

use crate::warn;

use super::dpx_mem::{new, renew};
use super::dpx_numbers::{tt_get_signed_pair, tt_get_unsigned_pair, tt_get_unsigned_quad};
use super::dpx_sfnt::{sfnt_find_table_pos, sfnt_locate_table, sfnt_set_table};
use super::dpx_tt_table::{
    tt_head_table, tt_hhea_table, tt_maxp_table, tt_os2__table, tt_pack_head_table,
    tt_pack_hhea_table, tt_pack_maxp_table, tt_read_head_table, tt_read_hhea_table,
    tt_read_longMetrics, tt_read_maxp_table, tt_read_os2__table, tt_read_vhea_table, tt_vhea_table,
};
use crate::dpx_truetype::sfnt_table_info;
use crate::qsort;
use crate::{ttstub_input_read, ttstub_input_seek};
use libc::{free, memcpy, memset};

pub type __ssize_t = i64;
pub type size_t = u64;
pub type ssize_t = __ssize_t;
pub type Fixed = u32;
pub type FWord = i16;
pub type uFWord = u16;

use super::dpx_sfnt::{put_big_endian, sfnt};

#[derive(Copy, Clone)]
#[repr(C)]
pub struct tt_glyph_desc {
    pub gid: u16,
    pub ogid: u16,
    pub advw: u16,
    pub advh: u16,
    pub lsb: i16,
    pub tsb: i16,
    pub llx: i16,
    pub lly: i16,
    pub urx: i16,
    pub ury: i16,
    pub length: u32,
    pub data: *mut u8,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct tt_glyphs {
    pub num_glyphs: u16,
    pub max_glyphs: u16,
    pub last_gid: u16,
    pub emsize: u16,
    pub dw: u16,
    pub default_advh: u16,
    pub default_tsb: i16,
    pub gd: *mut tt_glyph_desc,
    pub used_slot: *mut u8,
}

use super::dpx_tt_table::tt_longMetrics;

unsafe extern "C" fn find_empty_slot(mut g: *mut tt_glyphs) -> u16 {
    let mut gid: u16 = 0;
    assert!(!g.is_null());
    gid = 0_u16;
    while (gid as i32) < 65534i32 {
        if *(*g).used_slot.offset((gid as i32 / 8i32) as isize) as i32
            & 1i32 << 7i32 - gid as i32 % 8i32
            == 0
        {
            break;
        }
        gid = gid.wrapping_add(1)
    }
    if gid as i32 == 65534i32 {
        panic!("No empty glyph slot available.");
    }
    gid
}
#[no_mangle]
pub unsafe extern "C" fn tt_find_glyph(mut g: *mut tt_glyphs, mut gid: u16) -> u16 {
    let mut new_gid: u16 = 0_u16;
    assert!(!g.is_null());
    for idx in 0..(*g).num_glyphs as i32 {
        if gid as i32 == (*(*g).gd.offset(idx as isize)).ogid as i32 {
            new_gid = (*(*g).gd.offset(idx as isize)).gid;
            break;
        }
    }
    new_gid
}
#[no_mangle]
pub unsafe extern "C" fn tt_get_index(mut g: *mut tt_glyphs, mut gid: u16) -> u16 {
    let mut idx: u16 = 0;
    assert!(!g.is_null());
    idx = 0_u16;
    while (idx as i32) < (*g).num_glyphs as i32 {
        if gid as i32 == (*(*g).gd.offset(idx as isize)).gid as i32 {
            break;
        }
        idx = idx.wrapping_add(1)
    }
    if idx as i32 == (*g).num_glyphs as i32 {
        idx = 0_u16
    }
    idx
}
#[no_mangle]
pub unsafe extern "C" fn tt_add_glyph(
    mut g: *mut tt_glyphs,
    mut gid: u16,
    mut new_gid: u16,
) -> u16 {
    assert!(!g.is_null());
    if *(*g).used_slot.offset((new_gid as i32 / 8i32) as isize) as i32
        & 1i32 << 7i32 - new_gid as i32 % 8i32
        != 0
    {
        warn!("Slot {} already used.", new_gid);
    } else {
        if (*g).num_glyphs as i32 + 1i32 >= 65534i32 {
            panic!("Too many glyphs.");
        }
        if (*g).num_glyphs as i32 >= (*g).max_glyphs as i32 {
            (*g).max_glyphs = ((*g).max_glyphs as i32 + 256i32) as u16;
            (*g).gd = renew(
                (*g).gd as *mut libc::c_void,
                ((*g).max_glyphs as u32 as u64)
                    .wrapping_mul(::std::mem::size_of::<tt_glyph_desc>() as u64)
                    as u32,
            ) as *mut tt_glyph_desc
        }
        (*(*g).gd.offset((*g).num_glyphs as isize)).gid = new_gid;
        (*(*g).gd.offset((*g).num_glyphs as isize)).ogid = gid;
        (*(*g).gd.offset((*g).num_glyphs as isize)).length = 0_u32;
        let ref mut fresh0 = (*(*g).gd.offset((*g).num_glyphs as isize)).data;
        *fresh0 = 0 as *mut u8;
        let ref mut fresh1 = *(*g).used_slot.offset((new_gid as i32 / 8i32) as isize);
        *fresh1 = (*fresh1 as i32 | 1i32 << 7i32 - new_gid as i32 % 8i32) as u8;
        (*g).num_glyphs = ((*g).num_glyphs as i32 + 1i32) as u16
    }
    if new_gid as i32 > (*g).last_gid as i32 {
        (*g).last_gid = new_gid
    }
    new_gid
}
/*
 * Initialization
 */
#[no_mangle]
pub unsafe extern "C" fn tt_build_init() -> *mut tt_glyphs {
    let mut g: *mut tt_glyphs = 0 as *mut tt_glyphs;
    g = new((1_u64).wrapping_mul(::std::mem::size_of::<tt_glyphs>() as u64) as u32)
        as *mut tt_glyphs;
    (*g).num_glyphs = 0_u16;
    (*g).max_glyphs = 0_u16;
    (*g).last_gid = 0_u16;
    (*g).emsize = 1_u16;
    (*g).default_advh = 0_u16;
    (*g).default_tsb = 0_i16;
    (*g).gd = 0 as *mut tt_glyph_desc;
    (*g).used_slot =
        new((8192_u64).wrapping_mul(::std::mem::size_of::<u8>() as u64) as u32) as *mut u8;
    memset((*g).used_slot as *mut libc::c_void, 0i32, 8192);
    tt_add_glyph(g, 0_u16, 0_u16);
    g
}
#[no_mangle]
pub unsafe extern "C" fn tt_build_finish(mut g: *mut tt_glyphs) {
    if !g.is_null() {
        if !(*g).gd.is_null() {
            for idx in 0..(*g).num_glyphs as i32 {
                free((*(*g).gd.offset(idx as isize)).data as *mut libc::c_void);
            }
            free((*g).gd as *mut libc::c_void);
        }
        free((*g).used_slot as *mut libc::c_void);
        free(g as *mut libc::c_void);
    };
}
#[inline]
unsafe extern "C" fn glyf_cmp(mut v1: *const libc::c_void, mut v2: *const libc::c_void) -> i32 {
    let mut cmp: i32 = 0i32;
    let mut sv1: *const tt_glyph_desc = 0 as *const tt_glyph_desc;
    let mut sv2: *const tt_glyph_desc = 0 as *const tt_glyph_desc;
    sv1 = v1 as *const tt_glyph_desc;
    sv2 = v2 as *const tt_glyph_desc;
    if (*sv1).gid as i32 == (*sv2).gid as i32 {
        cmp = 0i32
    } else if ((*sv1).gid as i32) < (*sv2).gid as i32 {
        cmp = -1i32
    } else {
        cmp = 1i32
    }
    cmp
}
#[no_mangle]
pub unsafe extern "C" fn tt_build_tables(mut sfont: *mut sfnt, mut g: *mut tt_glyphs) -> i32 {
    let mut hmtx_table_data: *mut i8 = 0 as *mut i8;
    let mut loca_table_data: *mut i8 = 0 as *mut i8;
    let mut glyf_table_data: *mut i8 = 0 as *mut i8;
    let mut hmtx_table_size: u32 = 0;
    let mut loca_table_size: u32 = 0;
    let mut glyf_table_size: u32 = 0;
    /* some information available from other TrueType table */
    let mut head: *mut tt_head_table = 0 as *mut tt_head_table;
    let mut hhea: *mut tt_hhea_table = 0 as *mut tt_hhea_table;
    let mut maxp: *mut tt_maxp_table = 0 as *mut tt_maxp_table;
    let mut hmtx: *mut tt_longMetrics = 0 as *mut tt_longMetrics;
    let mut vmtx: *mut tt_longMetrics = 0 as *mut tt_longMetrics;
    let mut os2: *mut tt_os2__table = 0 as *mut tt_os2__table;
    /* temp */
    let mut location: *mut u32 = 0 as *mut u32; /* Estimate most frequently appeared width */
    let mut offset: u32 = 0;
    let mut i: i32 = 0;
    let mut w_stat: *mut u16 = 0 as *mut u16;
    assert!(!g.is_null());
    if sfont.is_null() || (*sfont).handle.is_null() {
        panic!("File not opened.");
    }
    if (*sfont).type_0 != 1i32 << 0i32
        && (*sfont).type_0 != 1i32 << 4i32
        && (*sfont).type_0 != 1i32 << 8i32
    {
        panic!("Invalid font type");
    }
    if (*g).num_glyphs as i32 > 65534i32 {
        panic!("Too many glyphs.");
    }
    /*
     * Read head, hhea, maxp, loca:
     *
     *   unitsPerEm       --> head
     *   numHMetrics      --> hhea
     *   indexToLocFormat --> head
     *   numGlyphs        --> maxp
     */
    head = tt_read_head_table(sfont);
    hhea = tt_read_hhea_table(sfont);
    maxp = tt_read_maxp_table(sfont);
    if (*hhea).metricDataFormat as i32 != 0i32 {
        panic!("Unknown metricDataFormat.");
    }
    (*g).emsize = (*head).unitsPerEm;
    sfnt_locate_table(sfont, sfnt_table_info::HMTX);
    hmtx = tt_read_longMetrics(
        sfont,
        (*maxp).numGlyphs,
        (*hhea).numOfLongHorMetrics,
        (*hhea).numOfExSideBearings,
    );
    os2 = tt_read_os2__table(sfont);
    if !os2.is_null() {
        (*g).default_advh = ((*os2).sTypoAscender as i32 - (*os2).sTypoDescender as i32) as u16;
        (*g).default_tsb = ((*g).default_advh as i32 - (*os2).sTypoAscender as i32) as i16
    }
    if sfnt_find_table_pos(sfont, b"vmtx") > 0_u32 {
        let mut vhea: *mut tt_vhea_table = 0 as *mut tt_vhea_table;
        vhea = tt_read_vhea_table(sfont);
        sfnt_locate_table(sfont, b"vmtx");
        vmtx = tt_read_longMetrics(
            sfont,
            (*maxp).numGlyphs,
            (*vhea).numOfLongVerMetrics,
            (*vhea).numOfExSideBearings,
        );
        free(vhea as *mut libc::c_void);
    } else {
        vmtx = 0 as *mut tt_longMetrics
    }
    sfnt_locate_table(sfont, sfnt_table_info::LOCA);
    location = new((((*maxp).numGlyphs as i32 + 1i32) as u32 as u64)
        .wrapping_mul(::std::mem::size_of::<u32>() as u64) as u32) as *mut u32;
    if (*head).indexToLocFormat as i32 == 0i32 {
        for i in 0..=(*maxp).numGlyphs as i32 {
            *location.offset(i as isize) =
                (2_u32).wrapping_mul(tt_get_unsigned_pair((*sfont).handle) as u32);
        }
    } else if (*head).indexToLocFormat as i32 == 1i32 {
        for i in 0..=(*maxp).numGlyphs as i32 {
            *location.offset(i as isize) = tt_get_unsigned_quad((*sfont).handle);
        }
    } else {
        panic!("Unknown IndexToLocFormat.");
    }
    w_stat =
        new(((*g).emsize + 2).wrapping_mul(::std::mem::size_of::<u16>() as _) as _) as *mut u16;
    memset(
        w_stat as *mut libc::c_void,
        0i32,
        (::std::mem::size_of::<u16>()).wrapping_mul(((*g).emsize + 2) as _),
    );
    /*
     * Read glyf table.
     */
    offset = sfnt_locate_table(sfont, sfnt_table_info::GLYF);
    /*
     * The num_glyphs may grow when composite glyph is found.
     * A component of glyph refered by a composite glyph is appended
     * to used_glyphs if it is not already registered in used_glyphs.
     * Glyph programs of composite glyphs are modified so that it
     * correctly refer to new gid of their components.
     */
    /* old gid */
    for i in 0..65534 {
        let mut gid: u16 = 0;
        let mut loc: u32 = 0;
        let mut len: u32 = 0;
        let mut p: *mut u8 = 0 as *mut u8;
        let mut endptr: *mut u8 = 0 as *mut u8;
        let mut number_of_contours: i16 = 0;
        if i >= (*g).num_glyphs as i32 {
            break;
        }
        gid = (*(*g).gd.offset(i as isize)).ogid;
        if gid as i32 >= (*maxp).numGlyphs as i32 {
            panic!("Invalid glyph index (gid {})", gid);
        }
        loc = *location.offset(gid as isize);
        len = (*location.offset((gid as i32 + 1i32) as isize)).wrapping_sub(loc);
        (*(*g).gd.offset(i as isize)).advw = (*hmtx.offset(gid as isize)).advance;
        (*(*g).gd.offset(i as isize)).lsb = (*hmtx.offset(gid as isize)).sideBearing;
        if !vmtx.is_null() {
            (*(*g).gd.offset(i as isize)).advh = (*vmtx.offset(gid as isize)).advance;
            (*(*g).gd.offset(i as isize)).tsb = (*vmtx.offset(gid as isize)).sideBearing
        } else {
            (*(*g).gd.offset(i as isize)).advh = (*g).default_advh;
            (*(*g).gd.offset(i as isize)).tsb = (*g).default_tsb
        }
        (*(*g).gd.offset(i as isize)).length = len;
        let ref mut fresh2 = (*(*g).gd.offset(i as isize)).data;
        *fresh2 = 0 as *mut u8;
        if (*(*g).gd.offset(i as isize)).advw as i32 <= (*g).emsize as i32 {
            let ref mut fresh3 = *w_stat.offset((*(*g).gd.offset(i as isize)).advw as isize);
            *fresh3 = (*fresh3 as i32 + 1i32) as u16
        } else {
            let ref mut fresh4 = *w_stat.offset(((*g).emsize as i32 + 1i32) as isize);
            *fresh4 = (*fresh4 as i32 + 1i32) as u16
            /* larger than em */
        }
        if !(len == 0_u32) {
            if len < 10_u32 {
                panic!("Invalid TrueType glyph data (gid {}).", gid);
            }
            p = new((len as u64).wrapping_mul(::std::mem::size_of::<u8>() as u64) as u32)
                as *mut u8;
            let ref mut fresh5 = (*(*g).gd.offset(i as isize)).data;
            *fresh5 = p;
            endptr = p.offset(len as isize);
            ttstub_input_seek((*sfont).handle, offset.wrapping_add(loc) as ssize_t, 0i32);
            number_of_contours = tt_get_signed_pair((*sfont).handle);
            p = p.offset(
                put_big_endian(p as *mut libc::c_void, number_of_contours as i32, 2i32) as isize,
            );
            /* BoundingBox: FWord x 4 */
            (*(*g).gd.offset(i as isize)).llx = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).lly = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).urx = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).ury = tt_get_signed_pair((*sfont).handle);
            /* _FIXME_ */
            if vmtx.is_null() {
                /* vertOriginY == sTypeAscender */
                (*(*g).gd.offset(i as isize)).tsb = ((*g).default_advh as i32
                    - (*g).default_tsb as i32
                    - (*(*g).gd.offset(i as isize)).ury as i32)
                    as i16
            }
            p = p.offset(put_big_endian(
                p as *mut libc::c_void,
                (*(*g).gd.offset(i as isize)).llx as i32,
                2i32,
            ) as isize);
            p = p.offset(put_big_endian(
                p as *mut libc::c_void,
                (*(*g).gd.offset(i as isize)).lly as i32,
                2i32,
            ) as isize);
            p = p.offset(put_big_endian(
                p as *mut libc::c_void,
                (*(*g).gd.offset(i as isize)).urx as i32,
                2i32,
            ) as isize);
            p = p.offset(put_big_endian(
                p as *mut libc::c_void,
                (*(*g).gd.offset(i as isize)).ury as i32,
                2i32,
            ) as isize);
            /* Read evrything else. */
            ttstub_input_read(
                (*sfont).handle,
                p as *mut i8,
                len.wrapping_sub(10_u32) as size_t,
            );
            /*
             * Fix GIDs of composite glyphs.
             */
            if (number_of_contours as i32) < 0i32 {
                let mut flags: u16 = 0; /* flag, gid of a component */
                let mut cgid: u16 = 0;
                let mut new_gid: u16 = 0;
                loop {
                    if p >= endptr {
                        panic!("Invalid TrueType glyph data (gid {}): {} bytes", gid, len);
                    }
                    /*
                     * Flags and gid of component glyph are both u16.
                     */
                    flags = ((*p as i32) << 8i32 | *p.offset(1) as i32) as u16;
                    p = p.offset(2);
                    cgid = ((*p as i32) << 8i32 | *p.offset(1) as i32) as u16;
                    if cgid as i32 >= (*maxp).numGlyphs as i32 {
                        panic!(
                            "Invalid gid ({} > {}) in composite glyph {}.",
                            cgid,
                            (*maxp).numGlyphs,
                            gid,
                        );
                    }
                    new_gid = tt_find_glyph(g, cgid);
                    if new_gid as i32 == 0i32 {
                        new_gid = tt_add_glyph(g, cgid, find_empty_slot(g))
                    }
                    p = p.offset(
                        put_big_endian(p as *mut libc::c_void, new_gid as i32, 2i32) as isize
                    );
                    /*
                     * Just skip remaining part.
                     */
                    p = p.offset(
                        (if flags as i32 & 1i32 << 0i32 != 0 {
                            4i32
                        } else {
                            2i32
                        }) as isize,
                    );
                    if flags as i32 & 1i32 << 3i32 != 0 {
                        /* F2Dot14 */
                        p = p.offset(2)
                    } else if flags as i32 & 1i32 << 6i32 != 0 {
                        /* F2Dot14 x 2 */
                        p = p.offset(4)
                    } else if flags as i32 & 1i32 << 7i32 != 0 {
                        /* F2Dot14 x 4 */
                        p = p.offset(8)
                    }
                    if !(flags as i32 & 1i32 << 5i32 != 0) {
                        break;
                    }
                }
            }
        }
        /* Does not contains any data. */
    }
    free(location as *mut libc::c_void);
    free(hmtx as *mut libc::c_void);
    free(vmtx as *mut libc::c_void);
    let mut max_count: i32 = -1i32;
    (*g).dw = (*(*g).gd.offset(0)).advw;
    for i in 0..(*g).emsize as i32 + 1i32 {
        if *w_stat.offset(i as isize) as i32 > max_count {
            max_count = *w_stat.offset(i as isize) as i32;
            (*g).dw = i as u16
        }
    }
    free(w_stat as *mut libc::c_void);
    qsort(
        (*g).gd as *mut libc::c_void,
        (*g).num_glyphs as size_t,
        ::std::mem::size_of::<tt_glyph_desc>() as u64,
        Some(
            glyf_cmp as unsafe extern "C" fn(_: *const libc::c_void, _: *const libc::c_void) -> i32,
        ),
    );
    let mut prev: u16 = 0;
    let mut last_advw: u16 = 0;
    let mut p_0: *mut i8 = 0 as *mut i8;
    let mut q: *mut i8 = 0 as *mut i8;
    let mut padlen: i32 = 0;
    let mut num_hm_known: i32 = 0;
    glyf_table_size = 0u64 as u32;
    num_hm_known = 0i32;
    last_advw = (*(*g).gd.offset(((*g).num_glyphs as i32 - 1i32) as isize)).advw;
    i = (*g).num_glyphs as i32 - 1i32;
    while i >= 0i32 {
        padlen = (if (*(*g).gd.offset(i as isize)).length.wrapping_rem(4_u32) != 0 {
            (4_u32).wrapping_sub((*(*g).gd.offset(i as isize)).length.wrapping_rem(4_u32))
        } else {
            0_u32
        }) as i32;
        glyf_table_size = (glyf_table_size as u32).wrapping_add(
            (*(*g).gd.offset(i as isize))
                .length
                .wrapping_add(padlen as u32),
        ) as u32 as u32;
        if num_hm_known == 0 && last_advw as i32 != (*(*g).gd.offset(i as isize)).advw as i32 {
            (*hhea).numOfLongHorMetrics = ((*(*g).gd.offset(i as isize)).gid as i32 + 2i32) as u16;
            num_hm_known = 1i32
        }
        i -= 1
    }
    /* All advance widths are same. */
    if num_hm_known == 0 {
        (*hhea).numOfLongHorMetrics = 1_u16
    }
    hmtx_table_size =
        ((*hhea).numOfLongHorMetrics as i32 * 2i32 + ((*g).last_gid as i32 + 1i32) * 2i32) as u32;
    /*
     * Choosing short format does not always give good result
     * when compressed. Sometimes increases size.
     */
    if (glyf_table_size as u64) < 0x20000 {
        (*head).indexToLocFormat = 0_i16;
        loca_table_size = (((*g).last_gid as i32 + 2i32) * 2i32) as u32
    } else {
        (*head).indexToLocFormat = 1_i16;
        loca_table_size = (((*g).last_gid as i32 + 2i32) * 4i32) as u32
    }
    p_0 = new((hmtx_table_size as u64).wrapping_mul(::std::mem::size_of::<i8>() as u64) as u32)
        as *mut i8;
    hmtx_table_data = p_0;
    q = new((loca_table_size as u64).wrapping_mul(::std::mem::size_of::<i8>() as u64) as u32)
        as *mut i8;
    loca_table_data = q;
    glyf_table_data =
        new((glyf_table_size as u64).wrapping_mul(::std::mem::size_of::<i8>() as u64) as u32)
            as *mut i8;
    offset = 0u64 as u32;
    prev = 0_u16;
    for i in 0..(*g).num_glyphs as i32 {
        let mut gap: i32 = 0;
        gap = (*(*g).gd.offset(i as isize)).gid as i32 - prev as i32 - 1i32;
        for j in 1..=gap {
            if prev as i32 + j == (*hhea).numOfLongHorMetrics as i32 - 1i32 {
                p_0 = p_0.offset(
                    put_big_endian(p_0 as *mut libc::c_void, last_advw as i32, 2i32) as isize,
                )
            } else if prev as i32 + j < (*hhea).numOfLongHorMetrics as i32 {
                p_0 = p_0.offset(put_big_endian(p_0 as *mut libc::c_void, 0i32, 2i32) as isize)
            }
            p_0 = p_0.offset(put_big_endian(p_0 as *mut libc::c_void, 0i32, 2i32) as isize);
            if (*head).indexToLocFormat as i32 == 0i32 {
                q = q.offset(put_big_endian(
                    q as *mut libc::c_void,
                    offset.wrapping_div(2_u32) as u16 as i32,
                    2i32,
                ) as isize)
            } else {
                q = q.offset(put_big_endian(q as *mut libc::c_void, offset as i32, 4i32) as isize)
            }
        }
        padlen = (if (*(*g).gd.offset(i as isize)).length.wrapping_rem(4_u32) != 0 {
            (4_u32).wrapping_sub((*(*g).gd.offset(i as isize)).length.wrapping_rem(4_u32))
        } else {
            0_u32
        }) as i32;
        if ((*(*g).gd.offset(i as isize)).gid as i32) < (*hhea).numOfLongHorMetrics as i32 {
            p_0 = p_0.offset(put_big_endian(
                p_0 as *mut libc::c_void,
                (*(*g).gd.offset(i as isize)).advw as i32,
                2i32,
            ) as isize)
        }
        p_0 = p_0.offset(put_big_endian(
            p_0 as *mut libc::c_void,
            (*(*g).gd.offset(i as isize)).lsb as i32,
            2i32,
        ) as isize);
        if (*head).indexToLocFormat as i32 == 0i32 {
            q = q.offset(put_big_endian(
                q as *mut libc::c_void,
                offset.wrapping_div(2_u32) as u16 as i32,
                2i32,
            ) as isize)
        } else {
            q = q.offset(put_big_endian(q as *mut libc::c_void, offset as i32, 4i32) as isize)
        }
        memset(
            glyf_table_data.offset(offset as isize) as *mut libc::c_void,
            0i32,
            (*(*g).gd.offset(i as isize))
                .length
                .wrapping_add(padlen as _) as _,
        );
        memcpy(
            glyf_table_data.offset(offset as isize) as *mut libc::c_void,
            (*(*g).gd.offset(i as isize)).data as *const libc::c_void,
            (*(*g).gd.offset(i as isize)).length as _,
        );
        offset = (offset as u32).wrapping_add(
            (*(*g).gd.offset(i as isize))
                .length
                .wrapping_add(padlen as u32),
        ) as u32 as u32;
        prev = (*(*g).gd.offset(i as isize)).gid;
        /* free data here since it consume much memory */
        free((*(*g).gd.offset(i as isize)).data as *mut libc::c_void);
        (*(*g).gd.offset(i as isize)).length = 0_u32;
        let ref mut fresh6 = (*(*g).gd.offset(i as isize)).data;
        *fresh6 = 0 as *mut u8;
    }
    if (*head).indexToLocFormat as i32 == 0i32 {
        q = q.offset(put_big_endian(
            q as *mut libc::c_void,
            offset.wrapping_div(2_u32) as u16 as i32,
            2i32,
        ) as isize)
    } else {
        q = q.offset(put_big_endian(q as *mut libc::c_void, offset as i32, 4i32) as isize)
    }
    sfnt_set_table(
        sfont,
        sfnt_table_info::HMTX,
        hmtx_table_data as *mut libc::c_void,
        hmtx_table_size,
    );
    sfnt_set_table(
        sfont,
        sfnt_table_info::LOCA,
        loca_table_data as *mut libc::c_void,
        loca_table_size,
    );
    sfnt_set_table(
        sfont,
        sfnt_table_info::GLYF,
        glyf_table_data as *mut libc::c_void,
        glyf_table_size,
    );
    (*head).checkSumAdjustment = 0_u32;
    (*maxp).numGlyphs = ((*g).last_gid as i32 + 1i32) as u16;
    /* TODO */
    sfnt_set_table(
        sfont,
        sfnt_table_info::MAXP,
        tt_pack_maxp_table(maxp) as *mut libc::c_void,
        32u64 as u32,
    );
    sfnt_set_table(
        sfont,
        sfnt_table_info::HHEA,
        tt_pack_hhea_table(hhea) as *mut libc::c_void,
        36u64 as u32,
    );
    sfnt_set_table(
        sfont,
        sfnt_table_info::HEAD,
        tt_pack_head_table(head) as *mut libc::c_void,
        54u64 as u32,
    );
    free(maxp as *mut libc::c_void);
    free(hhea as *mut libc::c_void);
    free(head as *mut libc::c_void);
    free(os2 as *mut libc::c_void);
    0i32
}
/* GID in original font */
/* optimal value for DW */
/* default value */
/* default value */
#[no_mangle]
pub unsafe extern "C" fn tt_get_metrics(mut sfont: *mut sfnt, mut g: *mut tt_glyphs) -> i32 {
    let mut head: *mut tt_head_table = 0 as *mut tt_head_table;
    let mut hhea: *mut tt_hhea_table = 0 as *mut tt_hhea_table;
    let mut maxp: *mut tt_maxp_table = 0 as *mut tt_maxp_table;
    let mut hmtx: *mut tt_longMetrics = 0 as *mut tt_longMetrics;
    let mut vmtx: *mut tt_longMetrics = 0 as *mut tt_longMetrics;
    let mut os2: *mut tt_os2__table = 0 as *mut tt_os2__table;
    /* temp */
    let mut location: *mut u32 = 0 as *mut u32;
    let mut offset: u32 = 0;
    let mut w_stat: *mut u16 = 0 as *mut u16;
    assert!(!g.is_null());
    if sfont.is_null() || (*sfont).handle.is_null() {
        panic!("File not opened.");
    }
    if (*sfont).type_0 != 1i32 << 0i32
        && (*sfont).type_0 != 1i32 << 4i32
        && (*sfont).type_0 != 1i32 << 8i32
    {
        panic!("Invalid font type");
    }
    /*
     * Read head, hhea, maxp, loca:
     *
     *   unitsPerEm       --> head
     *   numHMetrics      --> hhea
     *   indexToLocFormat --> head
     *   numGlyphs        --> maxp
     */
    head = tt_read_head_table(sfont);
    hhea = tt_read_hhea_table(sfont);
    maxp = tt_read_maxp_table(sfont);
    if (*hhea).metricDataFormat as i32 != 0i32 {
        panic!("Unknown metricDataFormat.");
    }
    (*g).emsize = (*head).unitsPerEm;
    sfnt_locate_table(sfont, sfnt_table_info::HMTX);
    hmtx = tt_read_longMetrics(
        sfont,
        (*maxp).numGlyphs,
        (*hhea).numOfLongHorMetrics,
        (*hhea).numOfExSideBearings,
    );
    os2 = tt_read_os2__table(sfont);
    (*g).default_advh = ((*os2).sTypoAscender as i32 - (*os2).sTypoDescender as i32) as u16;
    (*g).default_tsb = ((*g).default_advh as i32 - (*os2).sTypoAscender as i32) as i16;
    if sfnt_find_table_pos(sfont, b"vmtx") > 0_u32 {
        let mut vhea: *mut tt_vhea_table = 0 as *mut tt_vhea_table;
        vhea = tt_read_vhea_table(sfont);
        sfnt_locate_table(sfont, b"vmtx");
        vmtx = tt_read_longMetrics(
            sfont,
            (*maxp).numGlyphs,
            (*vhea).numOfLongVerMetrics,
            (*vhea).numOfExSideBearings,
        );
        free(vhea as *mut libc::c_void);
    } else {
        vmtx = 0 as *mut tt_longMetrics
    }
    sfnt_locate_table(sfont, sfnt_table_info::LOCA);
    location = new((((*maxp).numGlyphs as i32 + 1i32) as u32 as u64)
        .wrapping_mul(::std::mem::size_of::<u32>() as u64) as u32) as *mut u32;
    if (*head).indexToLocFormat as i32 == 0i32 {
        for i in 0..=(*maxp).numGlyphs as u32 {
            *location.offset(i as isize) =
                (2_u32).wrapping_mul(tt_get_unsigned_pair((*sfont).handle) as u32);
        }
    } else if (*head).indexToLocFormat as i32 == 1i32 {
        for i in 0..=(*maxp).numGlyphs as u32 {
            *location.offset(i as isize) = tt_get_unsigned_quad((*sfont).handle);
        }
    } else {
        panic!("Unknown IndexToLocFormat.");
    }
    w_stat = new((((*g).emsize as i32 + 2i32) as u32 as u64)
        .wrapping_mul(::std::mem::size_of::<u16>() as u64) as u32) as *mut u16;
    memset(
        w_stat as *mut libc::c_void,
        0i32,
        (::std::mem::size_of::<u16>()).wrapping_mul((*g).emsize as usize + 2),
    );
    /*
     * Read glyf table.
     */
    offset = sfnt_locate_table(sfont, sfnt_table_info::GLYF); /* old gid */
    for i in 0..(*g).num_glyphs as u32 {
        let mut gid: u16 = 0;
        let mut loc: u32 = 0;
        let mut len: u32 = 0;
        gid = (*(*g).gd.offset(i as isize)).ogid;
        if gid as i32 >= (*maxp).numGlyphs as i32 {
            panic!("Invalid glyph index (gid {})", gid);
        }
        loc = *location.offset(gid as isize);
        len = (*location.offset((gid as i32 + 1i32) as isize)).wrapping_sub(loc);
        (*(*g).gd.offset(i as isize)).advw = (*hmtx.offset(gid as isize)).advance;
        (*(*g).gd.offset(i as isize)).lsb = (*hmtx.offset(gid as isize)).sideBearing;
        if !vmtx.is_null() {
            (*(*g).gd.offset(i as isize)).advh = (*vmtx.offset(gid as isize)).advance;
            (*(*g).gd.offset(i as isize)).tsb = (*vmtx.offset(gid as isize)).sideBearing
        } else {
            (*(*g).gd.offset(i as isize)).advh = (*g).default_advh;
            (*(*g).gd.offset(i as isize)).tsb = (*g).default_tsb
        }
        (*(*g).gd.offset(i as isize)).length = len;
        let ref mut fresh7 = (*(*g).gd.offset(i as isize)).data;
        *fresh7 = 0 as *mut u8;
        if (*(*g).gd.offset(i as isize)).advw as i32 <= (*g).emsize as i32 {
            let ref mut fresh8 = *w_stat.offset((*(*g).gd.offset(i as isize)).advw as isize);
            *fresh8 = (*fresh8 as i32 + 1i32) as u16
        } else {
            let ref mut fresh9 = *w_stat.offset(((*g).emsize as i32 + 1i32) as isize);
            *fresh9 = (*fresh9 as i32 + 1i32) as u16
            /* larger than em */
        }
        if !(len == 0_u32) {
            if len < 10_u32 {
                panic!("Invalid TrueType glyph data (gid {}).", gid);
            }
            ttstub_input_seek((*sfont).handle, offset.wrapping_add(loc) as ssize_t, 0i32);
            tt_get_signed_pair((*sfont).handle);
            /* BoundingBox: FWord x 4 */
            (*(*g).gd.offset(i as isize)).llx = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).lly = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).urx = tt_get_signed_pair((*sfont).handle);
            (*(*g).gd.offset(i as isize)).ury = tt_get_signed_pair((*sfont).handle);
            /* _FIXME_ */
            if vmtx.is_null() {
                /* vertOriginY == sTypeAscender */
                (*(*g).gd.offset(i as isize)).tsb = ((*g).default_advh as i32
                    - (*g).default_tsb as i32
                    - (*(*g).gd.offset(i as isize)).ury as i32)
                    as i16
            }
        }
        /* Does not contains any data. */
    }
    free(location as *mut libc::c_void);
    free(hmtx as *mut libc::c_void);
    free(maxp as *mut libc::c_void);
    free(hhea as *mut libc::c_void);
    free(head as *mut libc::c_void);
    free(os2 as *mut libc::c_void);
    free(vmtx as *mut libc::c_void);
    let mut max_count: i32 = -1i32;
    (*g).dw = (*(*g).gd.offset(0)).advw;
    for i in 0..((*g).emsize as i32 + 1i32) as u32 {
        if *w_stat.offset(i as isize) as i32 > max_count {
            max_count = *w_stat.offset(i as isize) as i32;
            (*g).dw = i as u16
        }
    }
    free(w_stat as *mut libc::c_void);
    0i32
}
