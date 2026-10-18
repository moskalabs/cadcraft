/* tslint:disable */
/* eslint-disable */

export class BimoveDoc {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * 그릴 거리를 만든다. `layout` 이 "" 면 모형공간.
     */
    buildFrame(layout: string, tolerance: number): BimoveFrame;
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
     */
    exec(id: string, params_json: string): string;
    /**
     * 배치 탭 이름들.
     */
    layoutNames(): string[];
    /**
     * DXF 바이트를 읽는다 (ASCII/바이너리 둘 다).
     */
    static openDxf(bytes: Uint8Array): BimoveDoc;
    /**
     * 사각형 선택. `crossing` = 걸치기만 해도 잡기 (오→왼 드래그).
     */
    pickWindow(layout: string, x0: number, y0: number, x1: number, y1: number, crossing: boolean): number;
    /**
     * 한 점을 찍어 엔티티를 고른다. `aperture` 는 **도면 단위** (픽셀 ÷ 줌).
     */
    pick(layout: string, x: number, y: number, aperture: number): string | undefined;
    /**
     * 픽셀로 그린다. `cx`,`cy` 는 화면 가운데 도면 좌표, `scale` 은 단위당 픽셀.
     * `bg` 는 0xRRGGBB. 크기가 0 이거나 터무니없으면 null.
     */
    renderPixels(layout: string, cx: number, cy: number, scale: number, w: number, h: number, bg: number): BimoveRaster | undefined;
    /**
     * 도면 판번호. 편집할 때마다 올라간다 — UI 가 "다시 그려야 하나" 를 이걸로 안다.
     */
    revision(): number;
    /**
     * 커서가 붙을 점. `osmode` 는 오토캐드 OSMODE 비트마스크.
     * `baseX`/`baseY` 가 NaN 이면 시작점 없음으로 본다 (수직·연장이 그걸 본다).
     */
    snap(layout: string, x: number, y: number, aperture: number, osmode: number, base_x: number, base_y: number): BimoveSnap | undefined;
}

export class BimoveFrame {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * `[minX, minY, maxX, maxY]`
     */
    bounds(): Float64Array;
    /**
     * 펼친 블록 수. 상한에 걸려 잘렸는지 보는 용도다.
     */
    expanded(): number;
    /**
     * 프리미티브 개수 (= prims().length / primStride).
     */
    primCount(): number;
    /**
     * 프리미티브 하나당 정수 개수. TS 의 읽는 루프가 이 값을 쓴다.
     */
    static primStride(): number;
    /**
     * `[kind, rgb, start, len, lw_um, flags]` 의 반복.
     */
    prims(): Int32Array;
    /**
     * 삼각형 좌표 (세 꼭짓점이 이어서 온다).
     */
    tris(): Float64Array;
    /**
     * x,y 가 번갈아 들어간 좌표 배열.
     */
    verts(): Float64Array;
}

export class BimoveRaster {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    height(): number;
    /**
     * `ImageData` 에 그대로 넣을 RGBA 바이트 (곱하지 않은 알파).
     */
    rgba(): Uint8Array;
    width(): number;
}

export class BimoveSnap {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * 걸린 OSMODE 비트 하나.
     */
    mode(): number;
    /**
     * "Endpoint" / "Midpoint" / "Perpendicular" … 마커 옆에 그대로 띄우면 된다.
     */
    name(): string;
    x(): number;
    y(): number;
}

/**
 * 폰트를 등록한다. 도면을 열기 **전에** 불러야 한다.
 */
export function registerFont(name: string, bytes: Uint8Array): void;

/**
 * 폴백 폰트 차례를 정한다 (쉼표 구분). 등록한 이름을 여기 올려야 쓰인다.
 */
export function setFallbackFonts(list: string): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_bimovedoc_free: (a: number, b: number) => void;
    readonly __wbg_bimoveframe_free: (a: number, b: number) => void;
    readonly __wbg_bimoveraster_free: (a: number, b: number) => void;
    readonly __wbg_bimovesnap_free: (a: number, b: number) => void;
    readonly bimovedoc_buildFrame: (a: number, b: number, c: number, d: number) => number;
    readonly bimovedoc_exec: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly bimovedoc_layoutNames: (a: number) => [number, number];
    readonly bimovedoc_openDxf: (a: number, b: number) => [number, number, number];
    readonly bimovedoc_pick: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly bimovedoc_pickWindow: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => number;
    readonly bimovedoc_renderPixels: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => number;
    readonly bimovedoc_revision: (a: number) => number;
    readonly bimovedoc_snap: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => number;
    readonly bimoveframe_bounds: (a: number) => [number, number];
    readonly bimoveframe_expanded: (a: number) => number;
    readonly bimoveframe_primCount: (a: number) => number;
    readonly bimoveframe_primStride: () => number;
    readonly bimoveframe_prims: (a: number) => [number, number];
    readonly bimoveframe_tris: (a: number) => [number, number];
    readonly bimoveframe_verts: (a: number) => [number, number];
    readonly bimoveraster_height: (a: number) => number;
    readonly bimoveraster_rgba: (a: number) => [number, number];
    readonly bimoveraster_width: (a: number) => number;
    readonly bimovesnap_mode: (a: number) => number;
    readonly bimovesnap_name: (a: number) => [number, number];
    readonly bimovesnap_x: (a: number) => number;
    readonly bimovesnap_y: (a: number) => number;
    readonly registerFont: (a: number, b: number, c: number, d: number) => void;
    readonly setFallbackFonts: (a: number, b: number) => void;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_drop_slice: (a: number, b: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
