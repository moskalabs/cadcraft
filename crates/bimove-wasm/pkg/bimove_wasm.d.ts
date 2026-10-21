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
     * 읽은 치수 **도형** 수.
     *
     * 숫자가 맞는지는 이것만 봐서는 모른다 — 축척을 문서에서 못 찾으면
     * `notes` 에 그 얘기가 올라오므로 **둘을 같이** 봐야 한다.
     */
    dimCount(): number;
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
     * Bimove 문서(우리 포맷 JSON)를 읽는다. `page_id` 가 "" 면 첫 페이지.
     *
     * tldraw 로 저장된 도면을 엔진으로 그려보는 길이다. `pageIds`/`pageNames`
     * 로 다른 페이지를 고르고, **`notes` 로 안 그린 것을 받는다** — 해치와
     * Bimove 고유 도형(벽체·공간·치수…)은 아직 안 나온다.
     */
    static fromBimove(json: string, page_id: string): BimoveDoc;
    /**
     * Bimove 문서에서 만든 해치 개수.
     */
    hatchCount(): number;
    /**
     * 배치 탭 이름들.
     */
    layoutNames(): string[];
    /**
     * Bimove 문서에서 만든 선 개수.
     */
    lineCount(): number;
    /**
     * 옮기면서 **그리지 않은 것**. 비어 있으면 다 그렸다는 뜻이다.
     */
    notes(): string[];
    /**
     * DXF 바이트를 읽는다 (ASCII/바이너리 둘 다).
     */
    static openDxf(bytes: Uint8Array): BimoveDoc;
    /**
     * 읽은 문·창 **도형** 수.
     *
     * 하나가 엔티티 여럿이 된다 (벽을 가리는 사각형 + 잼브 둘 + 문짝 + 호).
     * 그래서 엔티티 수로는 몇 개가 들어왔는지 되짚을 수 없다.
     */
    openingCount(): number;
    /**
     * 지금 보고 있는 페이지 id. "" 를 넘겼으면 첫 페이지가 여기 들어 있다.
     */
    pageId(): string;
    /**
     * Bimove 문서의 페이지 id 들 (DXF 에서 열었으면 빈 배열).
     */
    pageIds(): string[];
    /**
     * 같은 차례의 페이지 이름들.
     */
    pageNames(): string[];
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
     * `draft` 면 **그릴 거리를 다시 만들지 않는다** — 쓰던 걸 그대로 쓴다.
     * 휠·드래그 중에 켜라. 60만 선분짜리에서 목록 다시 만들기가 프레임당
     * 150ms 라, 그 프레임이 통째로 밀리면서 직전 그림이 남아 보인다.
     * 손을 떼면 draft 를 끄고 한 번 더 그려서 제대로 맞춘다.
     */
    renderPixels(layout: string, cx: number, cy: number, scale: number, w: number, h: number, bg: number, draft: boolean): BimoveRaster | undefined;
    /**
     * 도면 판번호. 편집할 때마다 올라간다 — UI 가 "다시 그려야 하나" 를 이걸로 안다.
     */
    revision(): number;
    /**
     * 커서가 붙을 점. `osmode` 는 오토캐드 OSMODE 비트마스크.
     * `baseX`/`baseY` 가 NaN 이면 시작점 없음으로 본다 (수직·연장이 그걸 본다).
     */
    snap(layout: string, x: number, y: number, aperture: number, osmode: number, base_x: number, base_y: number): BimoveSnap | undefined;
    /**
     * Bimove 문서에서 만든 글자 개수.
     */
    textCount(): number;
    /**
     * 읽은 벽체 **도형** 수.
     */
    wallCount(): number;
    /**
     * 벽체로 만든 MLINE 수.
     *
     * `wallCount` 와 **같이** 띄워야 쓸모가 있다. 이어진 벽이 묶이면 이 쪽이
     * 훨씬 작고, 둘이 같으면 하나도 못 묶었다는 뜻이다 — 그러면 코너마다
     * 캡이 서서 "꺾이는 지점이 겹친" 옛 모양으로 돌아간다.
     */
    wallMlineCount(): number;
    /**
     * 읽은 공간 **도형** 수.
     *
     * 하나가 엔티티 넷이 된다 (면 + 테두리 + 이름 + 면적).
     */
    zoneCount(): number;
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
    readonly bimovedoc_dimCount: (a: number) => number;
    readonly bimovedoc_exec: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly bimovedoc_fromBimove: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly bimovedoc_hatchCount: (a: number) => number;
    readonly bimovedoc_layoutNames: (a: number) => [number, number];
    readonly bimovedoc_lineCount: (a: number) => number;
    readonly bimovedoc_notes: (a: number) => [number, number];
    readonly bimovedoc_openDxf: (a: number, b: number) => [number, number, number];
    readonly bimovedoc_openingCount: (a: number) => number;
    readonly bimovedoc_pageId: (a: number) => [number, number];
    readonly bimovedoc_pageIds: (a: number) => [number, number];
    readonly bimovedoc_pageNames: (a: number) => [number, number];
    readonly bimovedoc_pick: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly bimovedoc_pickWindow: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => number;
    readonly bimovedoc_renderPixels: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => number;
    readonly bimovedoc_revision: (a: number) => number;
    readonly bimovedoc_snap: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => number;
    readonly bimovedoc_textCount: (a: number) => number;
    readonly bimovedoc_wallCount: (a: number) => number;
    readonly bimovedoc_wallMlineCount: (a: number) => number;
    readonly bimovedoc_zoneCount: (a: number) => number;
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
