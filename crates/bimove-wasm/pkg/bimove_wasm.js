/* @ts-self-types="./bimove_wasm.d.ts" */

export class BimoveDoc {
    static __wrap(ptr) {
        const obj = Object.create(BimoveDoc.prototype);
        obj.__wbg_ptr = ptr;
        BimoveDocFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BimoveDocFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bimovedoc_free(ptr, 0);
    }
    /**
     * 그릴 거리를 만든다. `layout` 이 "" 면 모형공간.
     * @param {string} layout
     * @param {number} tolerance
     * @returns {BimoveFrame}
     */
    buildFrame(layout, tolerance) {
        const ptr0 = passStringToWasm0(layout, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_buildFrame(this.__wbg_ptr, ptr0, len0, tolerance);
        return BimoveFrame.__wrap(ret);
    }
    /**
     * 읽은 치수 **도형** 수.
     *
     * 숫자가 맞는지는 이것만 봐서는 모른다 — 축척을 문서에서 못 찾으면
     * `notes` 에 그 얘기가 올라오므로 **둘을 같이** 봐야 한다.
     * @returns {number}
     */
    dimCount() {
        const ret = wasm.bimovedoc_dimCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 엔진 명령을 실행한다. 결과 JSON 을 돌려주고, 실패하면 throw 한다.
     *
     * 명령을 하나씩 바인딩하지 않고 **문 하나**로 연다 — 엔진에 명령이 수백 개라
     * 하나씩 감싸면 끝이 없고, 늘 때마다 바인딩을 또 고쳐야 한다.
     *
     * ```js
     * doc.exec('line',  JSON.stringify({ points: [[0,0],[100,50]] }))
     * doc.exec('pline', JSON.stringify({ vertices: [[0,0],[10,0],[10,10]], closed: true }))
     * doc.exec('move',  JSON.stringify({ handles: [h], delta: [100, 0] }))
     * doc.exec('erase', JSON.stringify({ handles: [h] }))
     * doc.exec('u',     '{}')            // 되돌리기
     * doc.exec('redo',  '{}')            // 다시 실행
     * ```
     *
     * 되돌리기 스냅샷은 엔진이 알아서 쌓는다. 우리가 실행취소를 따로 만들
     * 필요가 없다 — `u` / `redo` 도 그냥 명령이다.
     * @param {string} id
     * @param {string} params_json
     * @returns {string}
     */
    exec(id, params_json) {
        let deferred4_0;
        let deferred4_1;
        try {
            const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(params_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ret = wasm.bimovedoc_exec(this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var ptr3 = ret[0];
            var len3 = ret[1];
            if (ret[3]) {
                ptr3 = 0; len3 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
        }
    }
    /**
     * Bimove 문서(우리 포맷 JSON)를 읽는다. `page_id` 가 "" 면 첫 페이지.
     *
     * tldraw 로 저장된 도면을 엔진으로 그려보는 길이다. `pageIds`/`pageNames`
     * 로 다른 페이지를 고르고, **`notes` 로 안 그린 것을 받는다** — 해치와
     * Bimove 고유 도형(벽체·공간·치수…)은 아직 안 나온다.
     * @param {string} json
     * @param {string} page_id
     * @returns {BimoveDoc}
     */
    static fromBimove(json, page_id) {
        const ptr0 = passStringToWasm0(json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(page_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_fromBimove(ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return BimoveDoc.__wrap(ret[0]);
    }
    /**
     * Bimove 문서에서 만든 해치 개수.
     * @returns {number}
     */
    hatchCount() {
        const ret = wasm.bimovedoc_hatchCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 배치 탭 이름들.
     * @returns {string[]}
     */
    layoutNames() {
        const ret = wasm.bimovedoc_layoutNames(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]);
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Bimove 문서에서 만든 선 개수.
     * @returns {number}
     */
    lineCount() {
        const ret = wasm.bimovedoc_lineCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 옮기면서 **그리지 않은 것**. 비어 있으면 다 그렸다는 뜻이다.
     * @returns {string[]}
     */
    notes() {
        const ret = wasm.bimovedoc_notes(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]);
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * DXF 바이트를 읽는다 (ASCII/바이너리 둘 다).
     * @param {Uint8Array} bytes
     * @returns {BimoveDoc}
     */
    static openDxf(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_openDxf(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return BimoveDoc.__wrap(ret[0]);
    }
    /**
     * 읽은 문·창 **도형** 수.
     *
     * 하나가 엔티티 여럿이 된다 (벽을 가리는 사각형 + 잼브 둘 + 문짝 + 호).
     * 그래서 엔티티 수로는 몇 개가 들어왔는지 되짚을 수 없다.
     * @returns {number}
     */
    openingCount() {
        const ret = wasm.bimovedoc_openingCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 지금 보고 있는 페이지 id. "" 를 넘겼으면 첫 페이지가 여기 들어 있다.
     * @returns {string}
     */
    pageId() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.bimovedoc_pageId(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Bimove 문서의 페이지 id 들 (DXF 에서 열었으면 빈 배열).
     * @returns {string[]}
     */
    pageIds() {
        const ret = wasm.bimovedoc_pageIds(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]);
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * 같은 차례의 페이지 이름들.
     * @returns {string[]}
     */
    pageNames() {
        const ret = wasm.bimovedoc_pageNames(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]);
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * 사각형 선택. `crossing` = 걸치기만 해도 잡기 (오→왼 드래그).
     * @param {string} layout
     * @param {number} x0
     * @param {number} y0
     * @param {number} x1
     * @param {number} y1
     * @param {boolean} crossing
     * @returns {number}
     */
    pickWindow(layout, x0, y0, x1, y1, crossing) {
        const ptr0 = passStringToWasm0(layout, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_pickWindow(this.__wbg_ptr, ptr0, len0, x0, y0, x1, y1, crossing);
        return ret >>> 0;
    }
    /**
     * 한 점을 찍어 엔티티를 고른다. `aperture` 는 **도면 단위** (픽셀 ÷ 줌).
     * @param {string} layout
     * @param {number} x
     * @param {number} y
     * @param {number} aperture
     * @returns {string | undefined}
     */
    pick(layout, x, y, aperture) {
        const ptr0 = passStringToWasm0(layout, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_pick(this.__wbg_ptr, ptr0, len0, x, y, aperture);
        let v2;
        if (ret[0] !== 0) {
            v2 = getStringFromWasm0(ret[0], ret[1]);
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v2;
    }
    /**
     * 픽셀로 그린다. `cx`,`cy` 는 화면 가운데 도면 좌표, `scale` 은 단위당 픽셀.
     * `bg` 는 0xRRGGBB. 크기가 0 이거나 터무니없으면 null.
     * `draft` 면 **그릴 거리를 다시 만들지 않는다** — 쓰던 걸 그대로 쓴다.
     * 휠·드래그 중에 켜라. 60만 선분짜리에서 목록 다시 만들기가 프레임당
     * 150ms 라, 그 프레임이 통째로 밀리면서 직전 그림이 남아 보인다.
     * 손을 떼면 draft 를 끄고 한 번 더 그려서 제대로 맞춘다.
     * @param {string} layout
     * @param {number} cx
     * @param {number} cy
     * @param {number} scale
     * @param {number} w
     * @param {number} h
     * @param {number} bg
     * @param {boolean} draft
     * @returns {BimoveRaster | undefined}
     */
    renderPixels(layout, cx, cy, scale, w, h, bg, draft) {
        const ptr0 = passStringToWasm0(layout, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_renderPixels(this.__wbg_ptr, ptr0, len0, cx, cy, scale, w, h, bg, draft);
        return ret === 0 ? undefined : BimoveRaster.__wrap(ret);
    }
    /**
     * 도면 판번호. 편집할 때마다 올라간다 — UI 가 "다시 그려야 하나" 를 이걸로 안다.
     * @returns {number}
     */
    revision() {
        const ret = wasm.bimovedoc_revision(this.__wbg_ptr);
        return ret;
    }
    /**
     * 커서가 붙을 점. `osmode` 는 오토캐드 OSMODE 비트마스크.
     * `baseX`/`baseY` 가 NaN 이면 시작점 없음으로 본다 (수직·연장이 그걸 본다).
     * @param {string} layout
     * @param {number} x
     * @param {number} y
     * @param {number} aperture
     * @param {number} osmode
     * @param {number} base_x
     * @param {number} base_y
     * @returns {BimoveSnap | undefined}
     */
    snap(layout, x, y, aperture, osmode, base_x, base_y) {
        const ptr0 = passStringToWasm0(layout, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bimovedoc_snap(this.__wbg_ptr, ptr0, len0, x, y, aperture, osmode, base_x, base_y);
        return ret === 0 ? undefined : BimoveSnap.__wrap(ret);
    }
    /**
     * Bimove 문서에서 만든 글자 개수.
     * @returns {number}
     */
    textCount() {
        const ret = wasm.bimovedoc_textCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 읽은 벽체 **도형** 수.
     * @returns {number}
     */
    wallCount() {
        const ret = wasm.bimovedoc_wallCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 벽체로 만든 MLINE 수.
     *
     * `wallCount` 와 **같이** 띄워야 쓸모가 있다. 이어진 벽이 묶이면 이 쪽이
     * 훨씬 작고, 둘이 같으면 하나도 못 묶었다는 뜻이다 — 그러면 코너마다
     * 캡이 서서 "꺾이는 지점이 겹친" 옛 모양으로 돌아간다.
     * @returns {number}
     */
    wallMlineCount() {
        const ret = wasm.bimovedoc_wallMlineCount(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BimoveDoc.prototype[Symbol.dispose] = BimoveDoc.prototype.free;

export class BimoveFrame {
    static __wrap(ptr) {
        const obj = Object.create(BimoveFrame.prototype);
        obj.__wbg_ptr = ptr;
        BimoveFrameFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BimoveFrameFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bimoveframe_free(ptr, 0);
    }
    /**
     * `[minX, minY, maxX, maxY]`
     * @returns {Float64Array}
     */
    bounds() {
        const ret = wasm.bimoveframe_bounds(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * 펼친 블록 수. 상한에 걸려 잘렸는지 보는 용도다.
     * @returns {number}
     */
    expanded() {
        const ret = wasm.bimoveframe_expanded(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 프리미티브 개수 (= prims().length / primStride).
     * @returns {number}
     */
    primCount() {
        const ret = wasm.bimoveframe_primCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * 프리미티브 하나당 정수 개수. TS 의 읽는 루프가 이 값을 쓴다.
     * @returns {number}
     */
    static primStride() {
        const ret = wasm.bimoveframe_primStride();
        return ret >>> 0;
    }
    /**
     * `[kind, rgb, start, len, lw_um, flags]` 의 반복.
     * @returns {Int32Array}
     */
    prims() {
        const ret = wasm.bimoveframe_prims(this.__wbg_ptr);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * 삼각형 좌표 (세 꼭짓점이 이어서 온다).
     * @returns {Float64Array}
     */
    tris() {
        const ret = wasm.bimoveframe_tris(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * x,y 가 번갈아 들어간 좌표 배열.
     * @returns {Float64Array}
     */
    verts() {
        const ret = wasm.bimoveframe_verts(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
}
if (Symbol.dispose) BimoveFrame.prototype[Symbol.dispose] = BimoveFrame.prototype.free;

export class BimoveRaster {
    static __wrap(ptr) {
        const obj = Object.create(BimoveRaster.prototype);
        obj.__wbg_ptr = ptr;
        BimoveRasterFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BimoveRasterFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bimoveraster_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    height() {
        const ret = wasm.bimoveraster_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * `ImageData` 에 그대로 넣을 RGBA 바이트 (곱하지 않은 알파).
     * @returns {Uint8Array}
     */
    rgba() {
        const ret = wasm.bimoveraster_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {number}
     */
    width() {
        const ret = wasm.bimoveraster_width(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BimoveRaster.prototype[Symbol.dispose] = BimoveRaster.prototype.free;

export class BimoveSnap {
    static __wrap(ptr) {
        const obj = Object.create(BimoveSnap.prototype);
        obj.__wbg_ptr = ptr;
        BimoveSnapFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BimoveSnapFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bimovesnap_free(ptr, 0);
    }
    /**
     * 걸린 OSMODE 비트 하나.
     * @returns {number}
     */
    mode() {
        const ret = wasm.bimovesnap_mode(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * "Endpoint" / "Midpoint" / "Perpendicular" … 마커 옆에 그대로 띄우면 된다.
     * @returns {string}
     */
    name() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.bimovesnap_name(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    x() {
        const ret = wasm.bimovesnap_x(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    y() {
        const ret = wasm.bimovesnap_y(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) BimoveSnap.prototype[Symbol.dispose] = BimoveSnap.prototype.free;

/**
 * 폰트를 등록한다. 도면을 열기 **전에** 불러야 한다.
 * @param {string} name
 * @param {Uint8Array} bytes
 */
export function registerFont(name, bytes) {
    const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    wasm.registerFont(ptr0, len0, ptr1, len1);
}

/**
 * 폴백 폰트 차례를 정한다 (쉼표 구분). 등록한 이름을 여기 올려야 쓰인다.
 * @param {string} list
 */
export function setFallbackFonts(list) {
    const ptr0 = passStringToWasm0(list, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    wasm.setFallbackFonts(ptr0, len0);
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_30c8987f7c2ed4e2: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./bimove_wasm_bg.js": import0,
    };
}

const BimoveDocFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bimovedoc_free(ptr, 1));
const BimoveFrameFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bimoveframe_free(ptr, 1));
const BimoveRasterFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bimoveraster_free(ptr, 1));
const BimoveSnapFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bimovesnap_free(ptr, 1));

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

function getArrayI32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__externref_drop_slice(ptr, len);
    return result;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

let cachedInt32ArrayMemory0 = null;
function getInt32ArrayMemory0() {
    if (cachedInt32ArrayMemory0 === null || cachedInt32ArrayMemory0.byteLength === 0) {
        cachedInt32ArrayMemory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
    cachedInt32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('bimove_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
