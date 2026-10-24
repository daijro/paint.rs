//#region src/byte-array.ts
function e(e) {
	return e instanceof Int8Array || e instanceof Uint8Array || e instanceof Uint8ClampedArray;
}
//#endregion
//#region src/configuration/configuration-file.ts
var t = class {
	fileName;
	data;
	constructor(e, t) {
		this.fileName = e, this.data = t;
	}
}, n = { XmlResourceFiles: {
	log: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE logmap [\n<!ELEMENT logmap (log)+>\n<!ELEMENT log (#PCDATA)>\n<!ATTLIST log events CDATA #IMPLIED>\n<!ATTLIST log output CDATA #IMPLIED>\n<!ATTLIST log filename CDATA #IMPLIED>\n<!ATTLIST log generations CDATA #IMPLIED>\n<!ATTLIST log limit CDATA #IMPLIED>\n<!ATTLIST log format CDATA #IMPLIED>\n]>\n<logmap>\n  <log events=\"None\"/>\n  <log output=\"Debug\"/>\n  <log filename=\"Magick-%g.log\"/>\n  <log generations=\"3\"/>\n  <log limit=\"2000\"/>\n  <log format=\"%t %r %u %v %d %c[%p]: %m/%f/%l/%d\n  %e\"/>\n</logmap>\n",
	policy: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE policymap [\n<!ELEMENT policymap (policy)*>\n<!ATTLIST policymap xmlns CDATA #FIXED \"\">\n<!ELEMENT policy EMPTY>\n<!ATTLIST policy xmlns CDATA #FIXED \"\">\n<!ATTLIST policy domain NMTOKEN #REQUIRED>\n<!ATTLIST policy name NMTOKEN #IMPLIED>\n<!ATTLIST policy pattern CDATA #IMPLIED>\n<!ATTLIST policy rights NMTOKEN #IMPLIED>\n<!ATTLIST policy stealth NMTOKEN #IMPLIED>\n<!ATTLIST policy value CDATA #IMPLIED>\n]>\n<policymap>\n  <policy domain=\"cache\" name=\"shared-secret\" value=\"passphrase\"/>\n  <policy domain=\"coder\" rights=\"none\" pattern=\"EPHEMERAL\" />\n  <policy domain=\"coder\" rights=\"none\" pattern=\"MVG\" />\n  <policy domain=\"coder\" rights=\"none\" pattern=\"MSL\" />\n  <policy domain=\"path\" rights=\"none\" pattern=\"@*\" />\n  <policy domain=\"path\" rights=\"none\" pattern=\"|*\" />\n</policymap>\n"
} }, r = class e {
	constructor() {
		this.log = new t("log.xml", n.XmlResourceFiles.log), this.policy = new t("policy.xml", n.XmlResourceFiles.policy);
	}
	static default = new e();
	*all() {
		yield this.log, yield this.policy;
	}
	log;
	policy;
}, i = class {
	constructor(e, t, n) {
		this.format = e, this.name = t, this.value = n;
	}
	format;
	name;
	value;
}, a = class {
	format;
	constructor(e) {
		this.format = e;
	}
	createDefine(e, t) {
		return typeof t == "boolean" ? new i(this.format, e, t ? "true" : "false") : typeof t == "string" ? new i(this.format, e, t) : new i(this.format, e, t.toString());
	}
	hasValue(e) {
		return e != null;
	}
}, o = class e {
	_scaleX;
	_scaleY;
	_shearX;
	_shearY;
	_translateX;
	_translateY;
	constructor(e = 1, t = 1, n = 0, r = 0, i = 0, a = 0) {
		this._scaleX = e, this._scaleY = t, this._shearX = n, this._shearY = r, this._translateX = i, this._translateY = a;
	}
	get scaleX() {
		return this._scaleX;
	}
	get scaleY() {
		return this._scaleY;
	}
	get shearX() {
		return this._shearX;
	}
	get shearY() {
		return this._shearY;
	}
	get translateX() {
		return this._translateX;
	}
	get translateY() {
		return this._translateY;
	}
	draw(e) {
		e.affine(this._scaleX, this._scaleY, this._shearX, this._shearY, this._translateX, this._translateY);
	}
	reset() {
		this._scaleX = 1, this._scaleY = 1, this._shearX = 0, this._shearY = 0, this._translateX = 0, this._translateY = 0;
	}
	transformOrigin(t, n) {
		let r = new e();
		r._translateX = t, r._translateY = n, this.transform(r);
	}
	transformRotation(t) {
		let n = new e();
		n._scaleX = Math.cos(e.normalizeAngleToRadians(t)), n._scaleY = Math.cos(e.normalizeAngleToRadians(t)), n._shearX = -Math.sin(e.normalizeAngleToRadians(t)), n._shearY = Math.sin(e.normalizeAngleToRadians(t)), this.transform(n);
	}
	transformScale(t, n) {
		let r = new e();
		r._scaleX = t, r._scaleY = n, this.transform(r);
	}
	transformSkewX(t) {
		let n = new e();
		n._shearX = Math.tan(e.normalizeAngleToRadians(t)), this.transform(n);
	}
	transformSkewY(t) {
		let n = new e();
		n._shearY = Math.tan(e.normalizeAngleToRadians(t)), this.transform(n);
	}
	static normalizeAngleToRadians(e) {
		let t = e / 360, n = Math.round(t);
		return Math.abs(t - n) === .5 && n % 2 != 0 && (n = t > 0 ? n - 1 : n + 1), Math.PI * (e - n * 360) / 180;
	}
	transform(e) {
		let t = this._scaleX, n = this._scaleY, r = this._shearX, i = this._shearY, a = this._translateX, o = this._translateY;
		this._scaleX = t * e._scaleX + i * e._shearX, this._scaleY = r * e._shearY + n * e._scaleY, this._shearX = r * e._scaleX + n * e._shearX, this._shearY = t * e._shearY + i * e._scaleY, this._translateX = t * e._translateX + i * e._translateY + a, this._translateY = r * e._translateX + n * e._translateY + o;
	}
}, s = class {
	_color;
	constructor(e) {
		this._color = e;
	}
	get color() {
		return this._color;
	}
	draw(e) {
		e.borderColor(this._color);
	}
}, c = class {
	_x;
	_y;
	_paintMethod;
	constructor(e, t, n) {
		this._x = e, this._y = t, this._paintMethod = n;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	get paintMethod() {
		return this._paintMethod;
	}
	draw(e) {
		e.color(this._x, this._y, this._paintMethod);
	}
}, l = class {
	_color;
	constructor(e) {
		this._color = e;
	}
	get color() {
		return this._color;
	}
	draw(e) {
		e.fillColor(this._color);
	}
}, u = class {
	_opacity;
	constructor(e) {
		this._opacity = e;
	}
	get opacity() {
		return this._opacity;
	}
	draw(e) {
		e.fillOpacity(this._opacity.toDouble() / 100);
	}
}, d = class {
	_fillRule;
	constructor(e) {
		this._fillRule = e;
	}
	get fillRule() {
		return this._fillRule;
	}
	draw(e) {
		e.fillRule(this._fillRule);
	}
}, f = class {
	_pointSize;
	constructor(e) {
		this._pointSize = e;
	}
	get pointSize() {
		return this._pointSize;
	}
	draw(e) {
		e.fontPointSize(this._pointSize);
	}
}, p = class {
	constructor(e, t) {
		this.eventType = e, this.message = t ?? "";
	}
	eventType;
	message;
}, m = {
	Undefined: 0,
	Activate: 1,
	Associate: 2,
	Background: 3,
	Copy: 4,
	Deactivate: 5,
	Discrete: 6,
	Disassociate: 7,
	Extract: 8,
	Off: 9,
	On: 10,
	Opaque: 11,
	Remove: 12,
	Set: 13,
	Shape: 14,
	Transparent: 15,
	OffIfOpaque: 16
}, h = {
	Red: 0,
	Cyan: 0,
	Gray: 0,
	Green: 1,
	Magenta: 1,
	Blue: 2,
	Yellow: 2,
	Black: 3,
	Alpha: 4,
	Index: 5,
	Meta0: 10,
	Meta1: 11,
	Meta2: 12,
	Meta3: 13,
	Meta4: 14,
	Meta5: 15,
	Meta6: 16,
	Meta7: 17,
	Meta8: 18,
	Meta9: 19,
	Meta10: 20,
	Meta11: 21,
	Meta12: 22,
	Meta13: 23,
	Meta14: 24,
	Meta15: 25,
	Meta16: 26,
	Meta17: 27,
	Meta18: 28,
	Meta19: 29,
	Meta20: 30,
	Meta21: 31,
	Meta22: 32,
	Meta23: 33,
	Meta24: 34,
	Meta25: 35,
	Meta26: 36,
	Meta27: 37,
	Meta28: 38,
	Meta29: 39,
	Meta30: 40,
	Meta31: 41,
	Meta32: 42,
	Meta33: 43,
	Meta34: 44,
	Meta35: 45,
	Meta36: 46,
	Meta37: 47,
	Meta38: 48,
	Meta39: 49,
	Meta40: 50,
	Meta41: 51,
	Meta42: 52,
	Meta43: 53,
	Meta44: 54,
	Meta45: 55,
	Meta46: 56,
	Meta47: 57,
	Meta48: 58,
	Meta49: 59,
	Meta50: 60,
	Meta51: 61,
	Meta52: 62,
	Composite: 64
}, g = {
	Undefined: 0,
	Red: 1,
	Gray: 1,
	Cyan: 1,
	Green: 2,
	Magenta: 2,
	Blue: 4,
	Yellow: 4,
	Black: 8,
	Alpha: 16,
	Opacity: 16,
	Index: 32,
	Composite: 31,
	TrueAlpha: 256,
	get RGB() {
		return this.Red | this.Green | this.Blue;
	},
	get CMYK() {
		return this.Cyan | this.Magenta | this.Yellow | this.Black;
	},
	get CMYKA() {
		return this.Cyan | this.Magenta | this.Yellow | this.Black | this.Alpha;
	},
	Meta0: 1 << h.Meta0,
	Meta1: 1 << h.Meta1,
	Meta2: 1 << h.Meta2,
	Meta3: 1 << h.Meta3,
	Meta4: 1 << h.Meta4,
	Meta5: 1 << h.Meta5,
	Meta6: 1 << h.Meta6,
	Meta7: 1 << h.Meta7,
	Meta8: 1 << h.Meta8,
	Meta9: 1 << h.Meta9,
	Meta10: 1 << h.Meta10,
	Meta11: 1 << h.Meta11,
	Meta12: 1 << h.Meta12,
	Meta13: 1 << h.Meta13,
	Meta14: 1 << h.Meta14,
	Meta15: 1 << h.Meta15,
	Meta16: 1 << h.Meta16,
	Meta17: 1 << h.Meta17,
	Meta18: 1 << h.Meta18,
	Meta19: 1 << h.Meta19,
	Meta20: 1 << h.Meta20,
	Meta21: 1 << h.Meta21,
	All: 134217727
}, ee = class {
	constructor(e, t, n, r) {
		this.red = e, this.green = t, this.blue = n, this.white = r;
	}
	red;
	green;
	blue;
	white;
}, _ = {
	Undefined: 0,
	CMY: 1,
	CMYK: 2,
	Gray: 3,
	HCL: 4,
	HCLp: 5,
	HSB: 6,
	HSI: 7,
	HSL: 8,
	HSV: 9,
	HWB: 10,
	Lab: 11,
	LCH: 12,
	LCHab: 13,
	LCHuv: 14,
	Log: 15,
	LMS: 16,
	Luv: 17,
	OHTA: 18,
	Rec601YCbCr: 19,
	Rec709YCbCr: 20,
	RGB: 21,
	scRGB: 22,
	sRGB: 23,
	Transparent: 24,
	XyY: 25,
	XYZ: 26,
	YCbCr: 27,
	YCC: 28,
	YDbDr: 29,
	YIQ: 30,
	YPbPr: 31,
	YUV: 32,
	LinearGray: 33,
	Jzazbz: 34,
	DisplayP3: 35,
	Adobe98: 36,
	ProPhoto: 37,
	Oklab: 38,
	Oklch: 39,
	CAT02LMSC: 40
}, te = {
	[_.Undefined]: "Undefined",
	[_.CMY]: "CMY",
	[_.CMYK]: "CMYK",
	[_.Gray]: "Gray",
	[_.HCL]: "HCL",
	[_.HCLp]: "HCLp",
	[_.HSB]: "HSB",
	[_.HSI]: "HSI",
	[_.HSL]: "HSL",
	[_.HSV]: "HSV",
	[_.HWB]: "HWB",
	[_.Lab]: "Lab",
	[_.LCH]: "LCH",
	[_.LCHab]: "LCHab",
	[_.LCHuv]: "LCHuv",
	[_.Log]: "Log",
	[_.LMS]: "LMS",
	[_.Luv]: "Luv",
	[_.OHTA]: "OHTA",
	[_.Rec601YCbCr]: "Rec601YCbCr",
	[_.Rec709YCbCr]: "Rec709YCbCr",
	[_.RGB]: "RGB",
	[_.scRGB]: "scRGB",
	[_.sRGB]: "sRGB",
	[_.Transparent]: "Transparent",
	[_.XyY]: "XyY",
	[_.XYZ]: "XYZ",
	[_.YCbCr]: "YCbCr",
	[_.YCC]: "YCC",
	[_.YDbDr]: "YDbDr",
	[_.YIQ]: "YIQ",
	[_.YPbPr]: "YPbPr",
	[_.YUV]: "YUV",
	[_.LinearGray]: "LinearGray",
	[_.Jzazbz]: "Jzazbz",
	[_.DisplayP3]: "DisplayP3",
	[_.Adobe98]: "Adobe98",
	[_.ProPhoto]: "ProPhoto",
	[_.Oklab]: "Oklab",
	[_.Oklch]: "Oklch",
	[_.CAT02LMSC]: "CAT02LMS"
}, ne = class {
	colorSpace = _.Undefined;
	copyright = null;
	description = null;
	manufacturer = null;
	model = null;
}, re = class {
	_data;
	_index;
	constructor(e) {
		this._data = e, this._index = 0, this.isLittleEndian = !1;
	}
	get index() {
		return this._index;
	}
	isLittleEndian;
	readLong() {
		return this.canRead(4) ? this.isLittleEndian ? this.readLongLSB() : this.readLongMSB() : null;
	}
	readString(e) {
		if (e == 0) return "";
		if (!this.canRead(e)) return null;
		let t = new TextDecoder("utf-8").decode(this._data.subarray(this._index, this._index + e)), n = t.indexOf("\0");
		return n != -1 && (t = t.substring(0, n)), this._index += e, t;
	}
	seek(e) {
		return e >= this._data.length ? !1 : (this._index = e, !0);
	}
	skip(e) {
		return this._index + e >= this._data.length ? !1 : (this._index += e, !0);
	}
	canRead(e) {
		return e > this._data.length ? !1 : this._index + e <= this._data.length;
	}
	readLongLSB() {
		let e = this._data[this._index];
		return e |= this._data[this._index + 1] << 8, e |= this._data[this._index + 2] << 16, e |= this._data[this._index + 3] << 24, this._index += 4, e;
	}
	readLongMSB() {
		let e = this._data[this._index] << 24;
		return e |= this._data[this._index + 1] << 16, e |= this._data[this._index + 2] << 8, e |= this._data[this._index + 3], this._index += 4, e;
	}
}, v = class e {
	_data = new ne();
	_reader;
	constructor(e) {
		this._reader = new re(e);
	}
	static read(t) {
		let n = new e(t);
		return n.readColorSpace(), n.readTagTable(), n._data;
	}
	readColorSpace() {
		this._reader.seek(16);
		let e = this._reader.readString(4);
		e != null && (this._data.colorSpace = this.determineColorSpace(e.trimEnd()));
	}
	determineColorSpace(e) {
		switch (e) {
			case "CMY": return _.CMY;
			case "CMYK": return _.CMYK;
			case "GRAY": return _.Gray;
			case "HSL": return _.HSL;
			case "HSV": return _.HSV;
			case "Lab": return _.Lab;
			case "Luv": return _.Luv;
			case "RGB": return _.sRGB;
			case "XYZ": return _.XYZ;
			case "YCbr": return _.YCbCr;
			default: return _.Undefined;
		}
	}
	readTagTable() {
		if (!this._reader.seek(128)) return;
		let e = this._reader.readLong();
		if (e != null) for (let t = 0; t < e; t++) switch (this._reader.readLong()) {
			case 1668313716:
				this._data.copyright = this.readTag();
				break;
			case 1684370275:
				this._data.description = this.readTag();
				break;
			case 1684893284:
				this._data.manufacturer = this.readTag();
				break;
			case 1684890724:
				this._data.model = this.readTag();
				break;
			default: this._reader.skip(8);
		}
	}
	readTag() {
		let e = this._reader.readLong(), t = this._reader.readLong();
		if (e === null || t === null) return null;
		let n = this._reader.index;
		if (!this._reader.seek(e)) return null;
		let r = this.readTagValue(t);
		return this._reader.seek(n), r;
	}
	readTagValue(e) {
		switch (this._reader.readString(4)) {
			case "desc": return this.readTextDescriptionTypeValue();
			case "text": return this.readTextTypeValue(e);
			default: return null;
		}
	}
	readTextDescriptionTypeValue() {
		if (!this._reader.skip(4)) return null;
		let e = this._reader.readLong();
		return e == null ? null : this._reader.readString(e);
	}
	readTextTypeValue(e) {
		return this._reader.skip(4) ? this._reader.readString(e) : null;
	}
}, ie = class {
	constructor(e, t) {
		this.name = e, this.data = t;
	}
	name;
	data;
}, ae = class extends ie {
	_data;
	constructor(e) {
		super("icc", e);
	}
	get colorSpace() {
		return this.initialize(), this._data.colorSpace;
	}
	get copyright() {
		return this.initialize(), this._data.copyright;
	}
	get description() {
		return this.initialize(), this._data.description;
	}
	get manufacturer() {
		return this.initialize(), this._data.manufacturer;
	}
	get model() {
		return this.initialize(), this._data.model;
	}
	initialize() {
		this._data ||= v.read(this.data);
	}
}, oe = {
	HighRes: 0,
	Quantum: 1
}, se = class e {
	constructor(e, t) {
		this.distortion = e, this.difference = t;
	}
	difference;
	distortion;
	static _create(t, n) {
		return new e(t, n);
	}
}, ce = class {
	constructor(e) {
		this.metric = e;
	}
	metric;
	highlightColor;
	lowlightColor;
	masklightColor;
	_setArtifacts(e) {
		this.highlightColor !== void 0 && e.setArtifact("compare:highlight-color", this.highlightColor), this.lowlightColor !== void 0 && e.setArtifact("compare:lowlight-color", this.lowlightColor), this.masklightColor !== void 0 && e.setArtifact("compare:masklight-color", this.masklightColor);
	}
}, le = {
	Undefined: 0,
	Alpha: 1,
	Atop: 2,
	Blend: 3,
	Blur: 4,
	Bumpmap: 5,
	ChangeMask: 6,
	Clear: 7,
	ColorBurn: 8,
	ColorDodge: 9,
	Colorize: 10,
	CopyBlack: 11,
	CopyBlue: 12,
	Copy: 13,
	CopyCyan: 14,
	CopyGreen: 15,
	CopyMagenta: 16,
	CopyAlpha: 17,
	CopyRed: 18,
	CopyYellow: 19,
	Darken: 20,
	DarkenIntensity: 21,
	Difference: 22,
	Displace: 23,
	Dissolve: 24,
	Distort: 25,
	DivideDst: 26,
	DivideSrc: 27,
	DstAtop: 28,
	Dst: 29,
	DstIn: 30,
	DstOut: 31,
	DstOver: 32,
	Exclusion: 33,
	HardLight: 34,
	HardMix: 35,
	Hue: 36,
	In: 37,
	Intensity: 38,
	Lighten: 39,
	LightenIntensity: 40,
	LinearBurn: 41,
	LinearDodge: 42,
	LinearLight: 43,
	Luminize: 44,
	Mathematics: 45,
	MinusDst: 46,
	MinusSrc: 47,
	Modulate: 48,
	ModulusAdd: 49,
	ModulusSubtract: 50,
	Multiply: 51,
	No: 52,
	Out: 53,
	Over: 54,
	Overlay: 55,
	PegtopLight: 56,
	PinLight: 57,
	Plus: 58,
	Replace: 59,
	Saturate: 60,
	Screen: 61,
	SoftLight: 62,
	SrcAtop: 63,
	Src: 64,
	SrcIn: 65,
	SrcOut: 66,
	SrcOver: 67,
	Threshold: 68,
	VividLight: 69,
	Xor: 70,
	Stereo: 71,
	Freeze: 72,
	Interpolate: 73,
	Negate: 74,
	Reflect: 75,
	SoftBurn: 76,
	SoftDodge: 77,
	Stamp: 78,
	RMSE: 79,
	SaliencyBlend: 80,
	SeamlessBlend: 81
}, ue = {
	Warning: 300,
	ResourceLimitWarning: 300,
	TypeWarning: 305,
	OptionWarning: 310,
	DelegateWarning: 315,
	MissingDelegateWarning: 320,
	CorruptImageWarning: 325,
	FileOpenWarning: 330,
	BlobWarning: 335,
	StreamWarning: 340,
	CacheWarning: 345,
	CoderWarning: 350,
	FilterWarning: 352,
	ModuleWarning: 355,
	DrawWarning: 360,
	ImageWarning: 365,
	WandWarning: 370,
	RandomWarning: 375,
	XServerWarning: 380,
	MonitorWarning: 385,
	RegistryWarning: 390,
	ConfigureWarning: 395,
	PolicyWarning: 399,
	Error: 400,
	ResourceLimitError: 400,
	TypeError: 405,
	OptionError: 410,
	DelegateError: 415,
	MissingDelegateError: 420,
	CorruptImageError: 425,
	FileOpenError: 430,
	BlobError: 435,
	StreamError: 440,
	CacheError: 445,
	CoderError: 450,
	FilterError: 452,
	ModuleError: 455,
	DrawError: 460,
	ImageError: 465,
	WandError: 470,
	RandomError: 475,
	XServerError: 480,
	MonitorError: 485,
	RegistryError: 490,
	ConfigureError: 495,
	PolicyError: 499
}, y = class extends Error {
	_relatedErrors = [];
	constructor(e, t = ue.Error) {
		super(e), this.severity = t;
	}
	severity;
	get relatedErrors() {
		return this._relatedErrors;
	}
	_setRelatedErrors(e) {
		this._relatedErrors = e;
	}
}, de = class {
	static get depth() {
		return Number(R._api._Quantum_Depth_Get());
	}
	static get max() {
		return R._api._Quantum_Max_Get();
	}
};
//#endregion
//#region src/internal/native/string.ts
function b(e, t) {
	return e === R._api._NullPointer ? t ?? null : x(e);
}
function x(e) {
	return R._api.UTF8ToString(Number(e));
}
function S(e, t) {
	let n = b(t);
	return e._MagickMemory_Relinquish(t), n;
}
function fe(e, t, n) {
	let r = e.lengthBytesUTF8(t) + 1, i = e._malloc(r);
	try {
		return e.stringToUTF8(t, i, r), n(e._CastToSize(i));
	} finally {
		e._free(i);
	}
}
function C(e, t) {
	return e === null ? t(R._api._NullPointer) : fe(R._api, e, t);
}
//#endregion
//#region src/magick-color.ts
var w = class e {
	constructor(e, t, n, r, i) {
		if (e !== void 0) {
			if (typeof e == "string") {
				let t = R._api._NullPointer;
				try {
					t = R._api._MagickColor_Create(), C(e, (e) => {
						if (R._api._MagickColor_Initialize(t, e) === 0) throw new y("invalid color specified");
						this.initialize(t);
					});
				} finally {
					R._api._free(Number(t));
				}
			} else this.r = e, this.g = t ?? 0, this.b = n ?? 0, i === void 0 ? this.a = r ?? de.max : (this.k = r ?? 0, this.a = i, this.isCmyk = !0);
		}
	}
	r = 0;
	g = 0;
	b = 0;
	a = 0;
	k = 0;
	isCmyk = !1;
	static _create(t) {
		let n = new e();
		return n.initialize(t), n;
	}
	fuzzyEquals(e, t) {
		return e == this || this._use((n) => e._use((e) => R._api._MagickColor_FuzzyEquals(n, e, t._toQuantum()) === 1));
	}
	toShortString() {
		return this.a === de.max ? this.isCmyk ? `cmyka(${this.r},${this.g},${this.b},${this.k})` : `#${this.toHex(this.r)}${this.toHex(this.g)}${this.toHex(this.b)}` : this.toString();
	}
	toString() {
		return this.isCmyk ? `cmyka(${this.r},${this.g},${this.b},${this.k},${(this.a / de.max).toFixed(4)})` : `#${this.toHex(this.r)}${this.toHex(this.g)}${this.toHex(this.b)}${this.toHex(this.a)}`;
	}
	_use(e) {
		let t = R._api._NullPointer;
		try {
			return t = R._api._MagickColor_Create(), R._api._MagickColor_Red_Set(t, this.r), R._api._MagickColor_Green_Set(t, this.g), R._api._MagickColor_Blue_Set(t, this.b), R._api._MagickColor_Alpha_Set(t, this.a), this.isCmyk ? (R._api._MagickColor_Black_Set(t, this.k), R._api._MagickColor_IsCMYK_Set(t, 1)) : R._api._MagickColor_IsCMYK_Set(t, 0), e(t);
		} finally {
			R._api._MagickColor_Dispose(t);
		}
	}
	initialize(e) {
		this.r = R._api._MagickColor_Red_Get(e), this.g = R._api._MagickColor_Green_Get(e), this.b = R._api._MagickColor_Blue_Get(e), this.a = R._api._MagickColor_Alpha_Get(e), this.isCmyk = R._api._MagickColor_IsCMYK_Get(e) === 1, this.isCmyk && (this.k = R._api._MagickColor_Black_Get(e));
	}
	toHex(e) {
		return e.toString(16).padStart(2, "0");
	}
}, pe = /* @__PURE__ */ function(e) {
	return e[e.NoValue = 0] = "NoValue", e[e.PercentValue = 4096] = "PercentValue", e[e.IgnoreAspectRatio = 8192] = "IgnoreAspectRatio", e[e.Less = 16384] = "Less", e[e.Greater = 32768] = "Greater", e[e.FillArea = 65536] = "FillArea", e[e.LimitPixels = 131072] = "LimitPixels", e[e.AspectRatio = 1048576] = "AspectRatio", e;
}({});
//#endregion
//#region src/internal/native/size.ts
function T(e) {
	return R._api._CastToSize(e);
}
//#endregion
//#region src/types/magick-geometry.ts
var E = class e {
	_includeXyInToString;
	_width = 0;
	_height = 0;
	_x = 0;
	_y = 0;
	_aspectRatio = !1;
	_fillArea = !1;
	_greater = !1;
	_isPercentage = !1;
	_ignoreAspectRatio = !1;
	_less = !1;
	_limitPixels = !1;
	constructor(e, t, n, r) {
		if (typeof e == "string") {
			this._includeXyInToString = e.indexOf("+") >= 0 || e.indexOf("-") >= 0;
			let t = R._api._MagickGeometry_Create();
			try {
				C(e, (n) => {
					let r = R._api._MagickGeometry_Initialize(t, n);
					if (r === pe.NoValue) throw new y("invalid geometry specified");
					this.hasFlag(r, pe.AspectRatio) ? this.initializeFromAspectRation(t, e) : this.initialize(t, r);
				});
			} finally {
				R._api._MagickGeometry_Dispose(t);
			}
		} else {
			if (n !== void 0 && r !== void 0 ? (this._width = n, this._height = r, this._x = e, this._y = t ?? 0, this._includeXyInToString = !0) : (this._width = e, this._height = t ?? this._width, this._x = 0, this._y = 0, this._includeXyInToString = !1), this._width < 0) throw new y("negative width is not allowed");
			if (this._height < 0) throw new y("negative height is not allowed");
		}
	}
	get aspectRatio() {
		return this._aspectRatio;
	}
	get fillArea() {
		return this._fillArea;
	}
	set fillArea(e) {
		this._fillArea = e;
	}
	get greater() {
		return this._greater;
	}
	set greater(e) {
		this._greater = e;
	}
	get height() {
		return this._height;
	}
	set height(e) {
		this._height = e;
	}
	get ignoreAspectRatio() {
		return this._ignoreAspectRatio;
	}
	set ignoreAspectRatio(e) {
		this._ignoreAspectRatio = e;
	}
	get isPercentage() {
		return this._isPercentage;
	}
	set isPercentage(e) {
		this._isPercentage = e;
	}
	get less() {
		return this._less;
	}
	set less(e) {
		this._less = e;
	}
	get limitPixels() {
		return this._limitPixels;
	}
	set limitPixels(e) {
		this._limitPixels = e;
	}
	get width() {
		return this._width;
	}
	set width(e) {
		this._width = e;
	}
	get x() {
		return this._x;
	}
	set x(e) {
		this._x = e;
	}
	get y() {
		return this._y;
	}
	set y(e) {
		this._y = e;
	}
	toString() {
		if (this._aspectRatio) return this._width + ":" + this._height;
		let e = "";
		return this._width == 0 && this._height == 0 ? e += "0x0" : (this._width > 0 && (e += this._width.toString()), this._height > 0 ? e += "x" + this._height.toString() : e += "x"), (this._x != 0 || this._y != 0 || this._includeXyInToString) && (this._x >= 0 && (e += "+"), e += this._x, this.y >= 0 && (e += "+"), e += this.y), this._fillArea && (e += "^"), this._greater && (e += ">"), this._isPercentage && (e += "%"), this._ignoreAspectRatio && (e += "!"), this._less && (e += "<"), this._limitPixels && (e += "@"), e;
	}
	static _fromRectangle(t) {
		if (t === R._api._NullPointer) throw new y("unable to allocate memory");
		try {
			let n = Number(R._api._MagickRectangle_Width_Get(t)), r = Number(R._api._MagickRectangle_Height_Get(t)), i = Number(R._api._MagickRectangle_X_Get(t)), a = Number(R._api._MagickRectangle_Y_Get(t));
			return new e(i, a, n, r);
		} finally {
			R._api._MagickRectangle_Dispose(t);
		}
	}
	_toRectangle(e) {
		let t = R._api._MagickRectangle_Create();
		if (t === R._api._NullPointer) throw new y("unable to allocate memory");
		try {
			return R._api._MagickRectangle_Width_Set(t, T(this._width)), R._api._MagickRectangle_Height_Set(t, T(this._height)), R._api._MagickRectangle_X_Set(t, T(this._x)), R._api._MagickRectangle_Y_Set(t, T(this._y)), e(t);
		} finally {
			R._api._MagickRectangle_Dispose(t);
		}
	}
	initialize(e, t) {
		this._width = Number(R._api._MagickGeometry_Width_Get(e)), this._height = Number(R._api._MagickGeometry_Height_Get(e)), this._x = Number(R._api._MagickGeometry_X_Get(e)), this._y = Number(R._api._MagickGeometry_Y_Get(e)), this._ignoreAspectRatio = this.hasFlag(t, pe.IgnoreAspectRatio), this._isPercentage = this.hasFlag(t, pe.PercentValue), this._fillArea = this.hasFlag(t, pe.FillArea), this._greater = this.hasFlag(t, pe.Greater), this._less = this.hasFlag(t, pe.Less), this._limitPixels = this.hasFlag(t, pe.LimitPixels);
	}
	initializeFromAspectRation(e, t) {
		this._aspectRatio = !0;
		let n = t.split(":");
		this._width = this.parseNumber(n[0]), this._height = this.parseNumber(n[1]), this._x = Number(R._api._MagickGeometry_X_Get(e)), this._y = Number(R._api._MagickGeometry_Y_Get(e));
	}
	parseNumber(e) {
		let t = 0;
		for (; t < e.length && !this.isNumber(e[t]);) t++;
		let n = t;
		for (; t < e.length && this.isNumber(e[t]);) t++;
		return parseInt(e.substr(n, t - n));
	}
	isNumber(e) {
		return e >= "0" && e <= "9";
	}
	hasFlag(e, t) {
		return (e & t) === t;
	}
}, me = class e {
	constructor(e, t) {
		this.x = e, this.y = t ?? e;
	}
	x;
	y;
	static _create(t) {
		return t === R._api._NullPointer ? new e(0, 0) : new e(R._api._PointInfo_X_Get(t), R._api._PointInfo_Y_Get(t));
	}
}, he = class e {
	constructor(e) {
		this.area = Number(R._api._ConnectedComponent_GetArea(e)), this.centroid = me._create(R._api._ConnectedComponent_GetCentroid(e)), this.color = w._create(R._api._ConnectedComponent_GetColor(e)), this.height = Number(R._api._ConnectedComponent_GetHeight(e)), this.id = Number(R._api._ConnectedComponent_GetId(e)), this.width = Number(R._api._ConnectedComponent_GetWidth(e)), this.x = Number(R._api._ConnectedComponent_GetX(e)), this.y = Number(R._api._ConnectedComponent_GetY(e));
	}
	area;
	centroid;
	color;
	height;
	id;
	width;
	x;
	y;
	static _create(t, n) {
		let r = [];
		if (t === R._api._NullPointer) return r;
		for (let i = 0; i < n; i++) {
			let n = R._api._ConnectedComponent_GetInstance(t, T(i));
			n === R._api._NullPointer || R._api._ConnectedComponent_GetArea(n) < 2 ** -52 || r.push(new e(n));
		}
		return r;
	}
	toGeometry() {
		return new E(this.x, this.y, this.width, this.height);
	}
}, ge = class {
	angleThreshold;
	areaThreshold;
	circularityThreshold;
	connectivity;
	diameterThreshold;
	eccentricityThreshold;
	majorAxisThreshold;
	meanColor;
	minorAxisThreshold;
	perimeterThreshold;
	constructor(e) {
		this.connectivity = e;
	}
	_setArtifacts(e) {
		this.angleThreshold !== void 0 && e.setArtifact("connected-components:angle-threshold", this.angleThreshold.toString()), this.areaThreshold !== void 0 && e.setArtifact("connected-components:area-threshold", this.areaThreshold.toString()), this.circularityThreshold !== void 0 && e.setArtifact("connected-components:circularity-threshold", this.circularityThreshold.toString()), this.diameterThreshold !== void 0 && e.setArtifact("connected-components:diameter-threshold", this.diameterThreshold.toString()), this.eccentricityThreshold !== void 0 && e.setArtifact("connected-components:eccentricity-threshold", this.eccentricityThreshold.toString()), this.majorAxisThreshold !== void 0 && e.setArtifact("connected-components:major-axis-threshold", this.majorAxisThreshold.toString()), this.meanColor !== void 0 && e.setArtifact("connected-components:mean-color", this.meanColor.toString()), this.minorAxisThreshold !== void 0 && e.setArtifact("connected-components:minor-axis-threshold", this.minorAxisThreshold.toString()), this.perimeterThreshold !== void 0 && e.setArtifact("connected-components:perimeter-threshold", this.perimeterThreshold.toString());
	}
}, D = {
	Undefined: 0,
	PixelsPerInch: 1,
	PixelsPerCentimeter: 2
}, _e = class e {
	constructor(e, t, n) {
		t === void 0 ? (this.x = e, this.y = e, this.units = D.PixelsPerInch) : n === void 0 ? (this.x = e, this.y = e, this.units = t) : (this.x = e, this.y = t, this.units = n);
	}
	x;
	y;
	units;
	toString(t) {
		return t == this.units || t === D.Undefined || t === void 0 ? e.toString(this.x, this.y, t ?? D.Undefined) : this.units == D.PixelsPerCentimeter && t == D.PixelsPerInch ? e.toString(this.x * 2.54, this.y * 2.54, t) : e.toString(this.x / 2.54, this.y / 2.54, t);
	}
	static toString(e, t, n) {
		let r = `${e}x${t}`;
		switch (n) {
			case D.PixelsPerCentimeter:
				r += "cm";
				break;
			case D.PixelsPerInch: r += "inch";
		}
		return r;
	}
}, O = class e {
	static _disposeAfterExecution(t, n) {
		try {
			let r = n(t);
			return r instanceof Promise ? Promise.resolve(r).then((n) => (t.dispose(), e.checkResult(t, n), n)) : (t.dispose(), e.checkResult(t, r), r);
		} catch (e) {
			throw t.dispose(), e;
		}
	}
	static checkResult(e, t) {
		if (t === e) throw new y("The result of the function cannot be the instance that has been disposed.");
		return t;
	}
}, ve = class {
	_pointer;
	_bytes;
	_func;
	constructor(e, t, n) {
		this._pointer = e, this._func = n, this._bytes = R._api.HEAPU8.subarray(Number(e), Number(e) + t);
	}
	func(e) {
		return e._bytes === void 0 ? e._func(/* @__PURE__ */ new Uint8Array()) : e._func(e._bytes);
	}
	dispose() {
		this._pointer = R._api._MagickMemory_Relinquish(this._pointer);
	}
}, ye = class {
	instance;
	type;
	constructor(e, t) {
		this.instance = R._api._malloc(e), this.type = t, R._api.setValue(this.instance, 0, this.type);
	}
	free() {
		R._api._free(this.instance);
	}
	get ptr() {
		return R._api._CastToSize(this.instance);
	}
	get value() {
		return R._api._CastToSize(R._api.getValue(this.instance, this.type));
	}
}, be = class e extends ye {
	constructor() {
		super(R._api._PointerSize, "*");
	}
	static use(t) {
		let n = new e();
		try {
			return t(n);
		} finally {
			n.free();
		}
	}
}, k = class e {
	pointer;
	constructor(e) {
		this.pointer = e;
	}
	get ptr() {
		return this.pointer.ptr;
	}
	check(e, t) {
		return this.isError() ? t() : e();
	}
	static usePointer(t, n) {
		return be.use((r) => {
			let i = t(r.ptr);
			return e.checkException(r, i, n);
		});
	}
	static use(t, n) {
		return be.use((r) => {
			let i = t(new e(r));
			return e.checkException(r, i, n);
		});
	}
	static checkException(t, n, r) {
		if (!e.isRaised(t)) return n;
		let i = e.getErrorSeverity(t.value);
		return i >= ue.Error ? e.throw(t, i) : r === void 0 ? e.dispose(t) : r(e.createError(t.value, i)), n;
	}
	isError() {
		return e.isRaised(this.pointer) ? e.getErrorSeverity(this.pointer.value) >= ue.Error : !1;
	}
	static getErrorSeverity(e) {
		return R._api._MagickExceptionHelper_Severity(e);
	}
	static isRaised(e) {
		return e.value !== R._api._NullPointer;
	}
	static throw(t, n) {
		let r = e.createError(t.value, n);
		throw e.dispose(t), r;
	}
	static createError(t, n) {
		let r = new y(e.getMessage(t), n), i = R._api._MagickExceptionHelper_RelatedCount(t);
		if (i === R._api._NullPointer) return r;
		let a = [];
		for (let n = 0; n < i; n++) {
			let r = R._api._MagickExceptionHelper_Related(t, T(n)), i = e.getErrorSeverity(r), o = e.createError(r, i);
			a.push(o);
		}
		return r._setRelatedErrors(a), r;
	}
	static getMessage(e) {
		let t = R._api._MagickExceptionHelper_Message(e), n = R._api._MagickExceptionHelper_Description(e), r = b(t, "Unknown error");
		return n !== R._api._NullPointer && (r += `(${x(n)})`), r;
	}
	static dispose(e) {
		R._api._MagickExceptionHelper_Dispose(e.value);
	}
}, xe = class {
	disposeMethod;
	instance;
	constructor(e, t) {
		this.instance = e, this.disposeMethod = t;
	}
	get _instance() {
		if (this.instance !== R._api._NullPointer) return this.instance;
		throw new y("instance is disposed");
	}
	set _instance(e) {
		this.disposeInstance(this.instance), this.instance = e;
	}
	dispose() {
		this.instance = this.disposeInstance(this.instance);
	}
	_setInstance(e, t) {
		return t.check(() => this.instance !== R._api._NullPointer && (this.dispose(), this.instance = e, !0), () => (this.disposeInstance(e), !0));
	}
	disposeInstance(e) {
		return e > 0 && (this.onDispose !== void 0 && this.onDispose(), this.disposeMethod(e)), R._api._NullPointer;
	}
}, Se = class e {
	constructor(e, t, n, r, i, a, o) {
		this.ascent = e, this.descent = t, this.maxHorizontalAdvance = n, this.textHeight = r, this.textWidth = i, this.underlinePosition = a, this.underlineThickness = o;
	}
	ascent;
	descent;
	maxHorizontalAdvance;
	textHeight;
	textWidth;
	underlinePosition;
	underlineThickness;
	static _create(t) {
		if (t === R._api._NullPointer) return null;
		try {
			let n = R._api._TypeMetric_Ascent_Get(t), r = R._api._TypeMetric_Descent_Get(t), i = R._api._TypeMetric_MaxHorizontalAdvance_Get(t), a = R._api._TypeMetric_TextHeight_Get(t), o = R._api._TypeMetric_TextWidth_Get(t), s = R._api._TypeMetric_UnderlinePosition_Get(t), c = R._api._TypeMetric_UnderlineThickness_Get(t);
			return new e(n, r, i, a, o, s, c);
		} finally {
			R._api._TypeMetric_Dispose(t);
		}
	}
};
//#endregion
//#region src/internal/native/array.ts
function Ce(e, t) {
	if (e.byteLength === 0) throw new y("The specified array cannot be empty");
	let n = 0;
	try {
		return n = R._api._malloc(e.byteLength), R._api.HEAPU8.set(e, n), t(T(n));
	} finally {
		n !== 0 && R._api._free(n);
	}
}
function we(e, t) {
	if (e.length === 0) throw new y("The specified array cannot be empty");
	let n = e.length * 8, r = 0;
	try {
		r = R._api._malloc(n);
		let i = new ArrayBuffer(n), a = new Float64Array(i);
		for (let t = 0; t < e.length; t++) a[t] = e[t];
		return R._api.HEAPU8.set(new Int8Array(i), r), t(T(r));
	} finally {
		r !== 0 && R._api._free(r);
	}
}
function Te(e, t) {
	if (e.byteLength === 0) throw new y("The specified array cannot be empty");
	let n = 0;
	try {
		return n = R._api._malloc(e.byteLength), R._api.HEAPU8.set(e, n), t(T(n));
	} finally {
		n !== 0 && R._api._free(n);
	}
}
//#endregion
//#region src/drawing/drawing-wand.ts
var Ee = class e extends xe {
	constructor(e) {
		let t = e.settings._drawing._use((t) => R._api._DrawingWand_Create(e._instance, t._instance)), n = R._api._DrawingWand_Dispose;
		super(t, n);
	}
	affine(e, t, n, r, i, a) {
		k.usePointer((o) => {
			R._api._DrawingWand_Affine(this._instance, e, t, n, r, i, a, o);
		});
	}
	borderColor(e) {
		k.usePointer((t) => {
			e._use((e) => {
				R._api._DrawingWand_BorderColor(this._instance, e, t);
			});
		});
	}
	color(e, t, n) {
		k.usePointer((r) => {
			R._api._DrawingWand_Color(this._instance, e, t, n, r);
		});
	}
	draw(e) {
		e.forEach((e) => {
			e.draw(this);
		}), k.usePointer((e) => {
			R._api._DrawingWand_Render(this._instance, e);
		});
	}
	fillColor(e) {
		k.usePointer((t) => {
			e._use((e) => {
				R._api._DrawingWand_FillColor(this._instance, e, t);
			});
		});
	}
	fillOpacity(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_FillOpacity(this._instance, e, t);
		});
	}
	fillRule(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_FillRule(this._instance, e, t);
		});
	}
	font(e) {
		k.usePointer((t) => {
			C(e, (e) => {
				R._api._DrawingWand_Font(this._instance, e, t);
			});
		});
	}
	fontPointSize(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_FontPointSize(this._instance, e, t);
		});
	}
	fontTypeMetrics(e, t) {
		return k.usePointer((n) => C(e, (e) => {
			let r = R._api._DrawingWand_FontTypeMetrics(this._instance, e, +!!t, n);
			return Se._create(r);
		}));
	}
	gravity(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_Gravity(this._instance, e, t);
		});
	}
	line(e, t, n, r) {
		k.usePointer((i) => {
			R._api._DrawingWand_Line(this._instance, e, t, n, r, i);
		});
	}
	pathFinish() {
		k.usePointer((e) => {
			R._api._DrawingWand_PathFinish(this._instance, e);
		});
	}
	pathLineToAbs(e, t) {
		k.usePointer((n) => {
			R._api._DrawingWand_PathLineToAbs(this._instance, e, t, n);
		});
	}
	pathLineToRel(e, t) {
		k.usePointer((n) => {
			R._api._DrawingWand_PathLineToRel(this._instance, e, t, n);
		});
	}
	pathMoveToAbs(e, t) {
		k.usePointer((n) => {
			R._api._DrawingWand_PathMoveToAbs(this._instance, e, t, n);
		});
	}
	pathMoveToRel(e, t) {
		k.usePointer((n) => {
			R._api._DrawingWand_PathMoveToRel(this._instance, e, t, n);
		});
	}
	pathStart() {
		k.usePointer((e) => {
			R._api._DrawingWand_PathStart(this._instance, e);
		});
	}
	point(e, t) {
		k.usePointer((n) => {
			R._api._DrawingWand_Point(this._instance, e, t, n);
		});
	}
	rectangle(e, t, n, r) {
		k.usePointer((i) => {
			R._api._DrawingWand_Rectangle(this._instance, e, t, n, r, i);
		});
	}
	roundRectangle(e, t, n, r, i, a) {
		k.usePointer((o) => {
			R._api._DrawingWand_RoundRectangle(this._instance, e, t, n, r, i, a, o);
		});
	}
	strokeColor(e) {
		k.usePointer((t) => {
			e._use((e) => {
				R._api._DrawingWand_StrokeColor(this._instance, e, t);
			});
		});
	}
	strokeDashArray(e) {
		k.usePointer((t) => {
			we(e, (n) => {
				R._api._DrawingWand_StrokeDashArray(this._instance, n, T(e.length), t);
			});
		});
	}
	strokeDashOffset(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_StrokeDashOffset(this._instance, e, t);
		});
	}
	strokeWidth(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_StrokeWidth(this._instance, e, t);
		});
	}
	text(e, t, n) {
		k.usePointer((r) => {
			C(n, (n) => {
				R._api._DrawingWand_Text(this._instance, e, t, n, r);
			});
		});
	}
	textAlignment(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextAlignment(this._instance, e, t);
		});
	}
	textAntialias(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextAntialias(this._instance, +!!e, t);
		});
	}
	textDecoration(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextDecoration(this._instance, e, t);
		});
	}
	textInterlineSpacing(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextInterlineSpacing(this._instance, e, t);
		});
	}
	textInterwordspacing(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextInterwordSpacing(this._instance, e, t);
		});
	}
	textKerning(e) {
		k.usePointer((t) => {
			R._api._DrawingWand_TextKerning(this._instance, e, t);
		});
	}
	textUnderColor(e) {
		k.usePointer((t) => {
			e._use((e) => {
				R._api._DrawingWand_TextUnderColor(this._instance, e, t);
			});
		});
	}
	static _use(t, n) {
		let r = new e(t);
		return O._disposeAfterExecution(r, n);
	}
}, A = class e extends ye {
	constructor() {
		super(8, "double");
	}
	static use(t) {
		let n = new e();
		try {
			return t(n);
		} finally {
			n.free();
		}
	}
}, j = {
	Undefined: 0,
	Forget: 0,
	Northwest: 1,
	North: 2,
	Northeast: 3,
	West: 4,
	Center: 5,
	East: 6,
	Southwest: 7,
	South: 8,
	Southeast: 9
};
function* De(e) {
	for (let t of e) switch (t) {
		case j.North:
			yield "north";
			break;
		case j.Northeast:
			yield "north", yield "east";
			break;
		case j.Northwest:
			yield "north", yield "west";
			break;
		case j.East:
			yield "east";
			break;
		case j.West:
			yield "west";
			break;
		case j.South:
			yield "south";
			break;
		case j.Southeast:
			yield "south", yield "east";
			break;
		case j.Southwest: yield "south", yield "west";
	}
}
function Oe(e) {
	switch (e) {
		case j.North: return "north";
		case j.Northeast: return "northeast";
		case j.Northwest: return "northwest";
		case j.East: return "east";
		case j.West: return "west";
		case j.South: return "south";
		case j.Southeast: return "southeast";
		case j.Southwest: return "southwest";
		case j.Center: return "center";
		default: return;
	}
}
//#endregion
//#region src/types/magick-error-info.ts
var ke = class e {
	constructor(e, t, n) {
		this.meanErrorPerPixel = e, this.normalizedMeanError = t, this.normalizedMaximumError = n;
	}
	meanErrorPerPixel;
	normalizedMaximumError;
	normalizedMeanError;
	static _create(t) {
		let n = R._api._MagickImage_MeanErrorPerPixel_Get(t._instance), r = R._api._MagickImage_NormalizedMeanError_Get(t._instance), i = R._api._MagickImage_NormalizedMaximumError_Get(t._instance);
		return new e(n, r, i);
	}
}, Ae = {
	Unknown: "UNKNOWN",
	ThreeFr: "3FR",
	ThreeG2: "3G2",
	ThreeGp: "3GP",
	A: "A",
	Aai: "AAI",
	Ai: "AI",
	APng: "APNG",
	Art: "ART",
	Arw: "ARW",
	Ashlar: "ASHLAR",
	Ase: "ASE",
	Aseprite: "ASEPRITE",
	Avci: "AVCI",
	Avi: "AVI",
	Avif: "AVIF",
	Avs: "AVS",
	B: "B",
	Bayer: "BAYER",
	Bayera: "BAYERA",
	Bgr: "BGR",
	Bgra: "BGRA",
	Bgro: "BGRO",
	Bmp: "BMP",
	Bmp2: "BMP2",
	Bmp3: "BMP3",
	Brf: "BRF",
	C: "C",
	C2pa: "C2PA",
	Cal: "CAL",
	Cals: "CALS",
	Canvas: "CANVAS",
	Caption: "CAPTION",
	Cin: "CIN",
	Cip: "CIP",
	Clip: "CLIP",
	Cmyk: "CMYK",
	Cmyka: "CMYKA",
	Cr2: "CR2",
	Cr3: "CR3",
	Crw: "CRW",
	Cube: "CUBE",
	Cur: "CUR",
	Cut: "CUT",
	Data: "DATA",
	Dcm: "DCM",
	Dcr: "DCR",
	Dcraw: "DCRAW",
	Dcx: "DCX",
	Dds: "DDS",
	Dfont: "DFONT",
	Dng: "DNG",
	Dpx: "DPX",
	Dxt1: "DXT1",
	Dxt5: "DXT5",
	Epdf: "EPDF",
	Epi: "EPI",
	Eps: "EPS",
	Eps2: "EPS2",
	Eps3: "EPS3",
	Epsf: "EPSF",
	Epsi: "EPSI",
	Ept: "EPT",
	Ept2: "EPT2",
	Ept3: "EPT3",
	Erf: "ERF",
	Exr: "EXR",
	Farbfeld: "FARBFELD",
	Fax: "FAX",
	Ff: "FF",
	Fff: "FFF",
	File: "FILE",
	Fits: "FITS",
	Fl32: "FL32",
	Flv: "FLV",
	Fractal: "FRACTAL",
	Ftp: "FTP",
	Fts: "FTS",
	Ftxt: "FTXT",
	G: "G",
	G3: "G3",
	G4: "G4",
	Gif: "GIF",
	Gif87: "GIF87",
	Gradient: "GRADIENT",
	Gray: "GRAY",
	Graya: "GRAYA",
	Group4: "GROUP4",
	Hald: "HALD",
	Hdr: "HDR",
	Heic: "HEIC",
	Heif: "HEIF",
	Histogram: "HISTOGRAM",
	Hrz: "HRZ",
	Htm: "HTM",
	Html: "HTML",
	Http: "HTTP",
	Https: "HTTPS",
	Icb: "ICB",
	Ico: "ICO",
	Icon: "ICON",
	Icn: "ICN",
	Iiq: "IIQ",
	Info: "INFO",
	Inline: "INLINE",
	Ipl: "IPL",
	Isobrl: "ISOBRL",
	Isobrl6: "ISOBRL6",
	J2c: "J2C",
	J2k: "J2K",
	Jng: "JNG",
	Jnx: "JNX",
	Jp2: "JP2",
	Jpc: "JPC",
	Jpe: "JPE",
	Jpeg: "JPEG",
	Jpg: "JPG",
	Jpm: "JPM",
	Jps: "JPS",
	Jpt: "JPT",
	Json: "JSON",
	Jxl: "JXL",
	K: "K",
	K25: "K25",
	Kdc: "KDC",
	Kernel: "KERNEL",
	Label: "LABEL",
	M: "M",
	M2v: "M2V",
	M4v: "M4V",
	Mac: "MAC",
	Map: "MAP",
	Mask: "MASK",
	Mat: "MAT",
	Matte: "MATTE",
	Mdc: "MDC",
	Mef: "MEF",
	Miff: "MIFF",
	Mkv: "MKV",
	Mng: "MNG",
	Mono: "MONO",
	Mov: "MOV",
	Mos: "MOS",
	Mp4: "MP4",
	Mpc: "MPC",
	Mpeg: "MPEG",
	Mpg: "MPG",
	Mpo: "MPO",
	Mrw: "MRW",
	Msl: "MSL",
	Msvg: "MSVG",
	Mtv: "MTV",
	Mvg: "MVG",
	Nef: "NEF",
	Nrw: "NRW",
	Null: "NULL",
	O: "O",
	Ora: "ORA",
	Orf: "ORF",
	Otb: "OTB",
	Otf: "OTF",
	Pal: "PAL",
	Palm: "PALM",
	Pam: "PAM",
	Pango: "PANGO",
	Pattern: "PATTERN",
	Pbm: "PBM",
	Pcd: "PCD",
	Pcds: "PCDS",
	Pcl: "PCL",
	Pct: "PCT",
	Pcx: "PCX",
	Pdb: "PDB",
	Pdf: "PDF",
	Pdfa: "PDFA",
	Pef: "PEF",
	Pes: "PES",
	Pfa: "PFA",
	Pfb: "PFB",
	Pfm: "PFM",
	Pgm: "PGM",
	Pgx: "PGX",
	Phm: "PHM",
	Picon: "PICON",
	Pict: "PICT",
	Pix: "PIX",
	Pjpeg: "PJPEG",
	Plasma: "PLASMA",
	Png: "PNG",
	Png00: "PNG00",
	Png24: "PNG24",
	Png32: "PNG32",
	Png48: "PNG48",
	Png64: "PNG64",
	Png8: "PNG8",
	Pnm: "PNM",
	Pocketmod: "POCKETMOD",
	Ppm: "PPM",
	Ps: "PS",
	Ps2: "PS2",
	Ps3: "PS3",
	Psb: "PSB",
	Psd: "PSD",
	Ptif: "PTIF",
	Pwp: "PWP",
	Qoi: "QOI",
	R: "R",
	RadialGradient: "RADIAL-GRADIENT",
	Raf: "RAF",
	Ras: "RAS",
	Raw: "RAW",
	Rgb: "RGB",
	Rgb565: "RGB565",
	Rgba: "RGBA",
	Rgbo: "RGBO",
	Rgf: "RGF",
	Rla: "RLA",
	Rle: "RLE",
	Rmf: "RMF",
	Rw2: "RW2",
	Rwl: "RWL",
	Scr: "SCR",
	Screenshot: "SCREENSHOT",
	Sct: "SCT",
	Sf3: "SF3",
	Sfw: "SFW",
	Sgi: "SGI",
	Shtml: "SHTML",
	Six: "SIX",
	Sixel: "SIXEL",
	SparseColor: "SPARSE-COLOR",
	Sr2: "SR2",
	Srf: "SRF",
	Srw: "SRW",
	Stegano: "STEGANO",
	Sti: "STI",
	StrImg: "STRIMG",
	Sun: "SUN",
	Svg: "SVG",
	Svgz: "SVGZ",
	Text: "TEXT",
	Tga: "TGA",
	Thumbnail: "THUMBNAIL",
	Tif: "TIF",
	Tiff: "TIFF",
	Tiff64: "TIFF64",
	Tile: "TILE",
	Tim: "TIM",
	Tm2: "TM2",
	Ttc: "TTC",
	Ttf: "TTF",
	Txt: "TXT",
	Ubrl: "UBRL",
	Ubrl6: "UBRL6",
	Uil: "UIL",
	Uyvy: "UYVY",
	Vda: "VDA",
	Vicar: "VICAR",
	Vid: "VID",
	Viff: "VIFF",
	Vips: "VIPS",
	Vst: "VST",
	WebM: "WEBM",
	WebP: "WEBP",
	Wbmp: "WBMP",
	Wbinfo: "WBINFO",
	Wmv: "WMV",
	Wpg: "WPG",
	X3f: "X3F",
	Xbm: "XBM",
	Xc: "XC",
	Xcf: "XCF",
	Xpm: "XPM",
	Xps: "XPS",
	Xv: "XV",
	Y: "Y",
	Yaml: "YAML",
	Ycbcr: "YCBCR",
	Ycbcra: "YCBCRA",
	Yuv: "YUV"
}, M = {
	Undefined: 0,
	Coalesce: 1,
	CompareAny: 2,
	CompareClear: 3,
	CompareOverlay: 4,
	Dispose: 5,
	Optimize: 6,
	OptimizeImage: 7,
	OptimizePlus: 8,
	OptimizeTrans: 9,
	RemoveDups: 10,
	RemoveZero: 11,
	Composite: 12,
	Merge: 13,
	Flatten: 14,
	Mosaic: 15,
	Trimbounds: 16
}, je = class extends xe {
	constructor(e) {
		let t = R._api._DrawingSettings_Create(), n = R._api._DrawingSettings_Dispose;
		super(t, n);
		let r = e.affine;
		r !== void 0 && R._api._DrawingSettings_SetAffine(this._instance, r.scaleX, r.scaleY, r.shearX, r.shearY, r.translateX, r.translateY), e.borderColor !== void 0 && e.borderColor._use((e) => {
			R._api._DrawingSettings_BorderColor_Set(this._instance, e);
		}), e.fillColor !== void 0 && e.fillColor._use((e) => {
			R._api._DrawingSettings_FillColor_Set(this._instance, e);
		}), e.fillRule !== void 0 && R._api._DrawingSettings_FillRule_Set(this._instance, e.fillRule), e.font !== void 0 && C(it._getFontFileName(e.font), (e) => {
			R._api._DrawingSettings_Font_Set(this._instance, e);
		}), e.fontPointsize !== void 0 && R._api._DrawingSettings_FontPointsize_Set(this._instance, e.fontPointsize), e.strokeColor !== void 0 && e.strokeColor._use((e) => {
			R._api._DrawingSettings_StrokeColor_Set(this._instance, e);
		});
		let i = e.strokeDashArray;
		i !== void 0 && we(i, (e) => {
			R._api._DrawingSettings_SetStrokeDashArray(this._instance, e, T(i.length));
		}), e.strokeDashOffset !== void 0 && R._api._DrawingSettings_StrokeDashOffset_Set(this._instance, e.strokeDashOffset), e.strokeWidth !== void 0 && R._api._DrawingSettings_StrokeWidth_Set(this._instance, e.strokeWidth), e.textAntiAlias !== void 0 && R._api._DrawingSettings_TextAntiAlias_Set(this._instance, +!!e.textAntiAlias), e.textGravity !== void 0 && R._api._DrawingSettings_TextGravity_Set(this._instance, e.textGravity), e.textKerning !== void 0 && R._api._DrawingSettings_TextKerning_Set(this._instance, e.textKerning), e.textUnderColor !== void 0 && e.textUnderColor._use((e) => {
			R._api._DrawingSettings_TextUnderColor_Set(this._instance, e);
		});
	}
	setFillColor(e) {
		e === void 0 ? R._api._DrawingSettings_FillColor_Set(this._instance, R._api._NullPointer) : e._use((e) => {
			R._api._DrawingSettings_FillColor_Set(this._instance, e);
		});
	}
	setFillPattern(e) {
		k.usePointer((t) => {
			e === void 0 ? R._api._DrawingSettings_SetFillPattern(this._instance, R._api._NullPointer, t) : R._api._DrawingSettings_SetFillPattern(this._instance, e._instance, t);
		});
	}
}, Me = class {
	affine;
	borderColor;
	backgroundColor;
	fillColor;
	fillRule;
	font;
	fontPointsize;
	strokeColor;
	strokeDashArray;
	strokeDashOffset;
	strokeWidth;
	textAntiAlias;
	textGravity;
	textKerning;
	textUnderColor;
	_use(e) {
		let t = new je(this);
		return O._disposeAfterExecution(t, e);
	}
}, Ne = {
	Undefined: 0,
	EvenOdd: 1,
	NonZero: 2
}, Pe = class extends xe {
	constructor(e) {
		let t = R._api._MagickSettings_Create(), n = R._api._MagickSettings_Dispose;
		super(t, n), e._colorFuzz !== void 0 && R._api._MagickSettings_SetColorFuzz(this._instance, e._colorFuzz), e._fileName !== void 0 && C(e._fileName, (e) => {
			R._api._MagickSettings_SetFileName(this._instance, e);
		}), e._ping && R._api._MagickSettings_SetPing(this._instance, 1), e._quality !== void 0 && R._api._MagickSettings_SetQuality(this._instance, T(e._quality)), e.antiAlias !== void 0 && R._api._MagickSettings_AntiAlias_Set(this._instance, +!!e.antiAlias), e.backgroundColor !== void 0 && e.backgroundColor._use((e) => {
			R._api._MagickSettings_BackgroundColor_Set(this._instance, e);
		}), e.colorSpace !== void 0 && R._api._MagickSettings_ColorSpace_Set(this._instance, e.colorSpace), e.colorType !== void 0 && R._api._MagickSettings_ColorType_Set(this._instance, e.colorType), e.compression !== void 0 && R._api._MagickSettings_Compression_Set(this._instance, e.compression), e.debug !== void 0 && R._api._MagickSettings_Debug_Set(this._instance, +!!e.debug), e.density !== void 0 && C(e.density.toString(), (e) => {
			R._api._MagickSettings_Density_Set(this._instance, e);
		}), e.depth !== void 0 && R._api._MagickSettings_Depth_Set(this._instance, T(e.depth)), e.endian !== void 0 && R._api._MagickSettings_Endian_Set(this._instance, e.endian), e.fillColor !== void 0 && this.setOption("fill", e.fillColor.toString()), e.font !== void 0 && C(it._getFontFileName(e.font), (e) => {
			R._api._MagickSettings_SetFont(this._instance, e);
		}), e.fontPointsize !== void 0 && R._api._MagickSettings_FontPointsize_Set(this._instance, e.fontPointsize), e.format !== void 0 && C(e.format, (e) => {
			R._api._MagickSettings_Format_Set(this._instance, e);
		}), e.interlace !== void 0 && R._api._MagickSettings_Interlace_Set(this._instance, e.interlace), e.page !== void 0 && C(e.page.toString(), (e) => {
			R._api._MagickSettings_SetPage(this._instance, e);
		}), e.strokeColor !== void 0 && this.setOption("stroke", e.strokeColor.toString()), e.strokeWidth !== void 0 && this.setOption("strokeWidth", e.strokeWidth.toString()), e.textInterlineSpacing !== void 0 && this.setOption("interline-spacing", e.textInterlineSpacing.toString());
		for (let t in e._options) this.setOption(t, e._options[t]);
	}
	setOption(e, t) {
		C(e, (e) => {
			C(t, (t) => {
				R._api._MagickSettings_SetOption(this._instance, e, t);
			});
		});
	}
}, Fe = class e {
	_colorFuzz;
	_drawing = new Me();
	_fileName;
	_onArtifact;
	_options = {};
	_ping = !1;
	_quality;
	get affine() {
		return this._drawing.affine;
	}
	set affine(e) {
		this._drawing.affine = e;
	}
	antiAlias;
	backgroundColor;
	get borderColor() {
		return this._drawing.borderColor;
	}
	set borderColor(e) {
		this._drawing.borderColor = e;
	}
	colorSpace;
	colorType;
	compression;
	debug;
	density;
	depth;
	endian;
	get fillColor() {
		return this._drawing.fillColor;
	}
	set fillColor(e) {
		this.setDefineAndArtifact("fill", e?.toString()), this._drawing.fillColor = e;
	}
	get fillRule() {
		return this._drawing.fillRule ?? Ne.Undefined;
	}
	set fillRule(e) {
		this._drawing.fillRule = e;
	}
	get font() {
		return this._drawing.font;
	}
	set font(e) {
		this._drawing.font = e;
	}
	get fontPointsize() {
		return this._drawing.fontPointsize;
	}
	set fontPointsize(e) {
		this._drawing.fontPointsize = e;
	}
	format;
	interlace;
	page;
	get strokeColor() {
		return this._drawing.strokeColor;
	}
	set strokeColor(e) {
		this._drawing.strokeColor = e;
	}
	get strokeDashArray() {
		return this._drawing.strokeDashArray;
	}
	set strokeDashArray(e) {
		this._drawing.strokeDashArray = e;
	}
	get strokeDashOffset() {
		return this._drawing.strokeDashOffset;
	}
	set strokeDashOffset(e) {
		this._drawing.strokeDashOffset = e;
	}
	get strokeWidth() {
		return this._drawing.strokeWidth;
	}
	set strokeWidth(e) {
		this.setDefineAndArtifact("stroke", e?.toString()), this._drawing.strokeWidth = e;
	}
	get textAntiAlias() {
		return this._drawing.textAntiAlias;
	}
	set textAntiAlias(e) {
		this._drawing.textAntiAlias = e;
	}
	textInterlineSpacing;
	get textKerning() {
		return this._drawing.textKerning;
	}
	set textKerning(e) {
		this.setDefineAndArtifact("kerning", e?.toString()), this._drawing.textKerning = e;
	}
	get textGravity() {
		return this._drawing.textGravity;
	}
	set textGravity(e) {
		this.setDefineAndArtifact("gravity", Oe(e)), this._drawing.textGravity = e;
	}
	get textUnderColor() {
		return this._drawing.textUnderColor;
	}
	set textUnderColor(e) {
		this._drawing.textUnderColor = e;
	}
	getDefine(e, t) {
		return t === void 0 ? this._options[e] ?? null : this._options[`${e}:${t}`] ?? null;
	}
	removeDefine(e, t) {
		if (t === void 0) delete this._options[e];
		else {
			let n = this.parseDefine(e, t);
			delete this._options[n];
		}
	}
	setDefine(e, t, n) {
		if (n === void 0) this._options[e] = t;
		else {
			let r = this.parseDefine(e, t);
			typeof n == "string" ? this._options[r] = n : typeof n == "number" ? this._options[r] = n.toString() : this._options[r] = n ? "true" : "false";
		}
	}
	setDefines(e) {
		e.getDefines().forEach((e) => {
			e !== void 0 && this.setDefine(e.format, e.name, e.value);
		});
	}
	_clone() {
		let t = new e();
		return Object.assign(t, this), t;
	}
	_use(e) {
		let t = new Pe(this);
		return O._disposeAfterExecution(t, e);
	}
	parseDefine(e, t) {
		return e === Ae.Unknown ? t : `${e}:${t}`;
	}
	setDefineAndArtifact(e, t) {
		t === void 0 ? this.removeDefine(e) : this.setDefine(e, t), this._onArtifact !== void 0 && this._onArtifact(e, t);
	}
}, N = class extends Fe {
	constructor(e) {
		super(), Object.assign(this, e);
	}
	extractArea;
	frameIndex;
	frameCount;
	height;
	get syncImageWithExifProfile() {
		let e = this.getDefine("exif:sync-image");
		return e === null || e.toLowerCase() === "true";
	}
	set syncImageWithExifProfile(e) {
		this.setDefine("exif:sync-image", e.toString());
	}
	get syncImageWithTiffProperties() {
		let e = this.getDefine("tiff:sync-image");
		return e === null || e.toLowerCase() === "true";
	}
	set syncImageWithTiffProperties(e) {
		this.setDefine("tiff:sync-image", e.toString());
	}
	width;
	_use(e) {
		let t = new Pe(this), n = this.getSize();
		if (n !== "" && C(n, (e) => {
			R._api._MagickSettings_SetSize(t._instance, e);
		}), this.frameIndex !== void 0 || this.frameCount !== void 0) {
			let e = T(this.frameIndex ?? 0), n = T(this.frameCount ?? 1);
			R._api._MagickSettings_SetScene(t._instance, e), R._api._MagickSettings_SetNumberScenes(t._instance, n), C((this.frameCount === void 0 ? e.toString() : `${e}-${Number(e) + Number(n)}`).toString(), (e) => {
				R._api._MagickSettings_SetScenes(t._instance, e);
			});
		}
		return this.extractArea !== void 0 && C(this.extractArea.toString(), (e) => {
			R._api._MagickSettings_Extract_Set(t._instance, e);
		}), O._disposeAfterExecution(t, e);
	}
	getSize() {
		return this.width !== void 0 && this.height !== void 0 ? `${this.width}x${this.height}` : this.width === void 0 ? this.height === void 0 ? "" : `x${this.height}` : `${this.width}x`;
	}
}, P = {
	Undefined: 0,
	No: 1,
	Riemersma: 2,
	FloydSteinberg: 3
}, Ie = class extends xe {
	constructor(e) {
		let t = R._api._QuantizeSettings_Create(), n = R._api._QuantizeSettings_Dispose;
		super(t, n), R._api._QuantizeSettings_SetColors(this._instance, T(e.colors)), R._api._QuantizeSettings_SetColorSpace(this._instance, e.colorSpace), R._api._QuantizeSettings_SetDitherMethod(this._instance, e.ditherMethod ?? P.No), R._api._QuantizeSettings_SetMeasureErrors(this._instance, +!!e.measureErrors), R._api._QuantizeSettings_SetTreeDepth(this._instance, T(e.treeDepth));
	}
}, Le = class {
	constructor() {
		this.colors = 256, this.colorSpace = _.Undefined, this.ditherMethod = P.Riemersma, this.measureErrors = !1, this.treeDepth = 0;
	}
	colors;
	colorSpace;
	ditherMethod;
	measureErrors;
	treeDepth;
	_use(e) {
		let t = new Ie(this);
		return O._disposeAfterExecution(t, e);
	}
}, Re = class e {
	_image;
	_names = [];
	constructor(e) {
		this._image = e;
	}
	setArtifact(e, t) {
		this._names.push(e), this._image.setArtifact(e, t);
	}
	static use(t, n) {
		let r = new e(t);
		try {
			return n(r);
		} finally {
			r.dispose();
		}
	}
	dispose() {
		for (let e of this._names) this._image.removeArtifact(e);
	}
}, F = class e extends Array {
	constructor() {
		super();
	}
	static create(t) {
		let n = e.createObject();
		return t !== void 0 && n.read(t), n;
	}
	dispose() {
		let e = this.pop();
		for (; e !== void 0;) e.dispose(), e = this.pop();
	}
	appendHorizontally(e) {
		return this.createImage((e, t) => R._api._MagickImageCollection_Append(e, 0, t.ptr), e);
	}
	appendVertically(e) {
		return this.createImage((e, t) => R._api._MagickImageCollection_Append(e, 1, t.ptr), e);
	}
	clone(t) {
		return e.use((e) => {
			for (let t = 0; t < this.length; t++) e.push(Qe._clone(this[t]));
			return t(e);
		});
	}
	coalesce() {
		this.replaceImages((e, t) => R._api._MagickImageCollection_Coalesce(e, t.ptr));
	}
	combine(e, t) {
		let n = t, r = _.sRGB;
		return typeof e == "number" ? r = e : n = e, this.createImage((e, t) => R._api._MagickImageCollection_Combine(e, r, t.ptr), n);
	}
	complex(e, t) {
		return Re.use(this[0], (n) => (e._setArtifacts(n), this.createImage((t, n) => R._api._MagickImageCollection_Complex(t, e.complexOperator, n.ptr), t)));
	}
	deconstruct() {
		this.replaceImages((e, t) => R._api._MagickImageCollection_Deconstruct(e, t.ptr));
	}
	evaluate(e, t) {
		return this.createImage((t, n) => R._api._MagickImageCollection_Evaluate(t, T(e), n.ptr), t);
	}
	flatten(e) {
		return this.mergeImages(M.Flatten, e);
	}
	fx(e, t, n) {
		this.throwIfEmpty();
		let r = g.All, i = n;
		return typeof t == "number" ? r = t : i = t, C(e, (e) => this.createImage((t, n) => R._api._MagickImageCollection_Fx(t, e, T(r), n.ptr), i));
	}
	merge(e) {
		return this.mergeImages(M.Merge, e);
	}
	montage(t, n) {
		return this.throwIfEmpty(), this.attachImages((r) => {
			let i = t._use((e) => k.use((t) => {
				let n = R._api._MagickImageCollection_Montage(r, e._instance, t.ptr);
				return this.checkResult(n, t);
			}));
			return e._createFromImages(i, this.getSettings(), (e) => {
				let r = t.transparentColor;
				return r !== void 0 && e.forEach((e) => {
					e.transparent(r);
				}), e.merge(n);
			});
		});
	}
	morph(e) {
		if (this.length < 2) throw new y("operation requires at least two images");
		this.replaceImages((t, n) => R._api._MagickImageCollection_Morph(t, T(e), n.ptr));
	}
	mosaic(e) {
		return this.mergeImages(M.Mosaic, e);
	}
	optimize() {
		this.replaceImages((e, t) => R._api._MagickImageCollection_Optimize(e, t.ptr));
	}
	optimizePlus() {
		this.replaceImages((e, t) => R._api._MagickImageCollection_OptimizePlus(e, t.ptr));
	}
	optimizeTransparency() {
		this.throwIfEmpty(), this.attachImages((e) => {
			k.usePointer((t) => {
				R._api._MagickImageCollection_OptimizeTransparency(e, t);
			});
		});
	}
	ping(e, t) {
		this.readOrPing(!0, e, t);
	}
	polynomial(e, t) {
		return this.createImage((t, n) => we(e, (r) => R._api._MagickImageCollection_Polynomial(t, r, T(e.length), n.ptr)), t);
	}
	quantize(e) {
		this.throwIfEmpty();
		let t = e === void 0 ? new Le() : e;
		return this.attachImages((e) => {
			t._use((t) => {
				k.usePointer((n) => {
					R._api._MagickImageCollection_Quantize(e, t._instance, n);
				});
			});
		}), t.measureErrors ? ke._create(this[0]) : null;
	}
	read(e, t) {
		this.readOrPing(!1, e, t);
	}
	remap(e, t) {
		this.throwIfEmpty();
		let n = t === void 0 ? new Le() : t;
		this.attachImages((t) => {
			n._use((n) => {
				k.use((r) => {
					R._api._MagickImageCollection_Remap(t, n._instance, e._instance, r.ptr);
				});
			});
		});
	}
	resetPage() {
		this.forEach((e) => {
			e.resetPage();
		});
	}
	smushHorizontal(e, t) {
		return this.smush(e, !1, t);
	}
	smushVertical(e, t) {
		return this.smush(e, !0, t);
	}
	trimBounds() {
		this.mergeImages(M.Trimbounds, () => {});
	}
	static use(t) {
		let n = e.create();
		return O._disposeAfterExecution(n, t);
	}
	write(e, t) {
		this.throwIfEmpty();
		let n = R._api._NullPointer, r = 0, i = this[0], a = this.getSettings();
		t === void 0 ? (t = e, a.format = i.format) : a.format = e, k.use((e) => {
			be.use((t) => {
				a._use((i) => {
					this.attachImages((a) => {
						n = R._api._MagickImage_WriteBlob(a, i._instance, t.ptr, e.ptr), r = Number(t.value);
					});
				});
			});
		});
		let o = new ve(n, r, t);
		return O._disposeAfterExecution(o, o.func);
	}
	static _createFromImages(t, n, r) {
		let i = e.createObject();
		return i.addImages(t, n._clone()), r(i);
	}
	addImages(e, t) {
		t.format = Ae.Unknown;
		let n = e;
		for (; n !== R._api._NullPointer;) {
			let e = R._api._MagickImage_GetNext(n);
			R._api._MagickImage_SetNext(n, R._api._NullPointer), this.push(Qe._createFromImage(n, t)), n = e;
		}
	}
	attachImages(e) {
		try {
			for (let e = 0; e < this.length - 1; e++) R._api._MagickImage_SetNext(this[e]._instance, this[e + 1]._instance);
			return e(this[0]._instance);
		} finally {
			for (let e = 0; e < this.length - 1; e++) R._api._MagickImage_SetNext(this[e]._instance, R._api._NullPointer);
		}
	}
	checkResult(e, t) {
		return t.check(() => e, () => (R._api._MagickImageCollection_Dispose(e), R._api._NullPointer));
	}
	static createObject() {
		return Object.create(e.prototype);
	}
	createImage(e, t) {
		this.throwIfEmpty();
		let n = this.attachImages((t) => k.use((n) => {
			let r = e(t, n);
			return this.checkResult(r, n);
		}));
		return Qe._createFromImage(n, this.getSettings())._use(t);
	}
	getSettings() {
		return this[0]._getSettings()._clone();
	}
	mergeImages(e, t) {
		return this.createImage((t, n) => R._api._MagickImageCollection_Merge(t, T(e), n.ptr), t);
	}
	readOrPing(e, t, n) {
		this.dispose(), k.use((r) => {
			let i = n === void 0 ? new N() : new N(n);
			i._ping = e, typeof t == "string" ? (i._fileName = t, i._use((e) => {
				let t = R._api._MagickImageCollection_ReadFile(e._instance, r.ptr);
				this.addImages(t, i);
			})) : i._use((e) => {
				let n = t.byteLength, a = 0;
				try {
					a = R._api._malloc(n), R._api.HEAPU8.set(t, a);
					let o = T(a), s = R._api._MagickImageCollection_ReadBlob(e._instance, o, R._api._NullPointer, T(n), r.ptr);
					this.addImages(s, i);
				} finally {
					a !== 0 && R._api._free(a);
				}
			});
		});
	}
	replaceImages(e) {
		this.throwIfEmpty();
		let t = this.attachImages((t) => k.use((n) => {
			let r = e(t, n);
			return this.checkResult(r, n);
		})), n = this.getSettings()._clone();
		this.dispose(), this.addImages(t, n);
	}
	smush(e, t, n) {
		return this.createImage((n, r) => R._api._MagickImageCollection_Smush(n, T(e), +!!t, r.ptr), n);
	}
	throwIfEmpty() {
		if (this.length === 0) throw new y("operation requires at least one image");
	}
}, I = class e {
	_value;
	constructor(e) {
		this._value = e;
	}
	static _fromQuantum(t) {
		return new e(t / de.max * 100);
	}
	multiply(e) {
		return e * this._value / 100;
	}
	toDouble() {
		return this._value;
	}
	toString() {
		return `${parseFloat(this._value.toFixed(2))}%`;
	}
	_toQuantum() {
		return de.max * (this._value / 100);
	}
}, ze = class {
	static use(e, t, n) {
		let r = R._api._MagickRectangle_Create();
		try {
			R._api._MagickRectangle_X_Set(r, T(t.x)), R._api._MagickRectangle_Y_Set(r, T(t.y));
			let i = t.width, a = t.height;
			return t.isPercentage && (i = new I(t.width).multiply(e.width), a = new I(t.height).multiply(e.height)), R._api._MagickRectangle_Width_Set(r, T(i)), R._api._MagickRectangle_Height_Set(r, T(a)), n(r);
		} finally {
			R._api._MagickRectangle_Dispose(r);
		}
	}
}, Be = class {
	static _use(e, t, n) {
		let r = R._api._NullPointer;
		try {
			return r = R._api._OffsetInfo_Create(), R._api._PrimaryInfo_X_Set(r, e), R._api._PrimaryInfo_Y_Set(r, t), n(r);
		} finally {
			R._api._free(Number(r));
		}
	}
}, Ve = class {
	_values;
	constructor() {
		this._values = Array(7).fill(0);
	}
	get(e) {
		return this._values[e];
	}
	set(e, t) {
		this._values[e] = t;
	}
}, He = class e {
	_huPhashes = /* @__PURE__ */ new Map();
	_hash = "";
	channel;
	constructor(e, t, n) {
		if (this.channel = e, typeof n == "string") this.parseHash(t, n);
		else for (let e = 0; e < t.length; e++) {
			let r = new Ve();
			for (let t = 0; t < 7; t++) {
				let i = R._api._ChannelPerceptualHash_GetHuPhash(n, T(e), T(t));
				r.set(t, i);
			}
			this._huPhashes.set(t[e], r);
		}
	}
	huPhash(e, t) {
		if (t < 0 || t > 6) throw new y("Invalid index specified");
		let n = this._huPhashes.get(e);
		if (n === void 0) throw new y("Invalid color space specified");
		return n.get(t);
	}
	sumSquaredDistance(e) {
		let t = 0;
		return this._huPhashes.forEach((n, r) => {
			for (let i = 0; i < 7; i++) {
				let a = n.get(i), o = e.huPhash(r, i);
				t += (a - o) * (a - o);
			}
		}), t;
	}
	toString() {
		return this._hash == "" && this.setHash(), this._hash;
	}
	parseHash(t, n) {
		this._hash = n;
		let r = 0;
		for (let i of t) {
			let t = new Ve();
			for (let i = 0; i < 7; i++, r += 5) {
				let a = Number.parseInt(n.substring(r, r + 5), 16);
				if (isNaN(a)) throw new y("Invalid hash specified");
				let o = a / e.powerOfTen(a >> 17);
				a & 65536 && (o = -o), t.set(i, o);
			}
			this._huPhashes.set(i, t);
		}
	}
	static powerOfTen(e) {
		switch (e) {
			case 2: return 100;
			case 3: return 1e3;
			case 4: return 1e4;
			case 5: return 1e5;
			case 6: return 1e6;
			default: return 10;
		}
	}
	setHash() {
		this._hash = "", this._huPhashes.forEach((e) => {
			for (let t = 0; t < 7; t++) {
				let n = e.get(t), r = 0;
				for (; r < 7 && Math.abs(n * 10) < 65356;) n *= 10, r++;
				r <<= 1, r < 0 && (r |= 1), r = (r << 16) + Math.floor(n < 0 ? -(n - .5) : n + .5), this._hash += r.toString(16);
			}
		});
	}
}, Ue = class e {
	_red;
	_green;
	_blue;
	constructor(t, n, r) {
		if (typeof t == "string") {
			let r = n ?? e._defaultColorspaces();
			e._validateColorSpaces(r);
			let i = 35 * r.length;
			if (t.length !== 3 * i) throw new y("Invalid hash size");
			this._red = new He(h.Red, r, t.substring(0, i)), this._blue = new He(h.Blue, r, t.substring(i, i + i)), this._green = new He(h.Green, r, t.substring(i + i));
		} else this._red = t, this._green = n, this._blue = r;
	}
	static _create(t, n, r) {
		if (r === R._api._NullPointer) throw new y("The native operation failed to create an instance");
		let i = e.createChannel(t, n, r, h.Red), a = e.createChannel(t, n, r, h.Green), o = e.createChannel(t, n, r, h.Blue);
		return new e(i, a, o);
	}
	static _defaultColorspaces() {
		return [_.XyY, _.HSB];
	}
	static _validateColorSpaces(e) {
		if (e.length < 1 || e.length > 6) throw new y("Invalid number of colorspaces, the minimum is 1 and the maximum is 6");
		if (new Set(e).size !== e.length) throw new y("Specifying the same colorspace more than once is not allowed");
	}
	getChannel(e) {
		switch (e) {
			case h.Red: return this._red;
			case h.Green: return this._green;
			case h.Blue: return this._blue;
			default: return null;
		}
	}
	sumSquaredDistance(e) {
		let t = e.getChannel(h.Red), n = e.getChannel(h.Green), r = e.getChannel(h.Blue);
		if (t === null || n === null || r === null) throw new y("The other perceptual hash should contain a red, green and blue channel.");
		return this._red.sumSquaredDistance(t) + this._green.sumSquaredDistance(n) + this._blue.sumSquaredDistance(r);
	}
	toString() {
		return this._red.toString() + this._green.toString() + this._blue.toString();
	}
	static createChannel(e, t, n, r) {
		return new He(r, t, R._api._PerceptualHash_GetInstance(e._instance, n, T(r)));
	}
}, We = class e extends xe {
	image;
	constructor(e) {
		let t = k.usePointer((t) => R._api._PixelCollection_Create(e._instance, t)), n = R._api._PixelCollection_Dispose;
		super(t, n), this.image = e;
	}
	static _create(t) {
		return new e(t);
	}
	static _use(t, n) {
		let r = new e(t);
		return O._disposeAfterExecution(r, n);
	}
	static _map(t, n, r) {
		let i = new e(t);
		try {
			i.use(0, 0, t.width, t.height, n, (e) => {
				r(Number(e));
			});
		} finally {
			i.dispose();
		}
	}
	getArea(e, t, n, r) {
		return k.usePointer((i) => {
			let a = Number(R._api._PixelCollection_GetArea(this._instance, T(e), T(t), T(n), T(r), i)), o = Number(n * r * this.image.channelCount);
			return R._api.HEAPU8.subarray(a, a + o);
		});
	}
	getReadOnlyArea(e, t, n, r) {
		return k.usePointer((i) => {
			let a = Number(R._api._PixelCollection_GetReadOnlyArea(this.image._instance, T(e), T(t), T(n), T(r), i)), o = Number(n * r * this.image.channelCount);
			return R._api.HEAPU8.subarray(a, a + o);
		});
	}
	getChannelIndex(e) {
		return this.image._channelOffset(e);
	}
	getColor(e, t) {
		let n = this.getArea(e, t, 1, 1), r = Array.from(n), i = this.image._channelOffset(h.Index);
		if (i >= 0 && r.splice(i, 1), r.length === 0) return null;
		if (r.length === 1) return new w(r[0], r[0], r[0]);
		if (r.length === 2) return new w(r[0], r[0], r[0], r[1]);
		let a = this.image._channelOffset(h.Black) >= 0, o = this.image._channelOffset(h.Alpha) >= 0;
		return a ? r.length === 4 || !o ? new w(r[0], r[1], r[2], r[3], de.max) : new w(r[0], r[1], r[2], r[3], r[4]) : r.length === 3 || !o ? new w(r[0], r[1], r[2]) : new w(r[0], r[1], r[2], r[3]);
	}
	getPixel(e, t) {
		return this.getArea(e, t, 1, 1);
	}
	setArea(e, t, n, r, i) {
		k.usePointer((a) => {
			let o = i instanceof Uint8Array ? i : new Uint8Array(i);
			Te(o, (i) => {
				let s = T(o.length);
				R._api._PixelCollection_SetArea(this._instance, T(e), T(t), T(n), T(r), i, s, a);
			});
		});
	}
	setPixel(e, t, n) {
		n instanceof Uint8Array, this.setArea(e, t, 1, 1, n);
	}
	toByteArray(e, t, n, r, i) {
		return this.use(e, t, n, r, i, (e) => {
			if (e === R._api._NullPointer) return null;
			let t = Number(n * r * i.length), a = Number(e);
			return R._api.HEAPU8.slice(a, a + t);
		});
	}
	use(e, t, n, r, i, a) {
		return C(i, (i) => k.use((o) => {
			let s = R._api._PixelCollection_ToByteArray(this._instance, T(e), T(t), T(n), T(r), i, o.ptr);
			return o.check(() => {
				let e = a(s);
				return s = R._api._MagickMemory_Relinquish(s), e;
			}, () => (s = R._api._MagickMemory_Relinquish(s), null));
		}));
	}
}, Ge = {
	Undefined: 0,
	Average: 1,
	Brightness: 2,
	Lightness: 3,
	MS: 4,
	Rec601Luma: 5,
	Rec601Luminance: 6,
	Rec709Luma: 7,
	Rec709Luminance: 8,
	RMS: 9
}, Ke = {
	Undefined: 0,
	Average: 1,
	Average9: 2,
	Average16: 3,
	Background: 4,
	Bilinear: 5,
	Blend: 6,
	Catrom: 7,
	Integer: 8,
	Mesh: 9,
	Nearest: 10,
	Spline: 11
}, qe = class e {
	constructor(e, t, n) {
		this.x = e, this.y = t, this.z = n;
	}
	x;
	y;
	z;
	static _create(t) {
		return t === R._api._NullPointer ? new e(0, 0, 0) : new e(R._api._PrimaryInfo_X_Get(t), R._api._PrimaryInfo_Y_Get(t), R._api._PrimaryInfo_Z_Get(t));
	}
	_use(e) {
		let t = R._api._NullPointer;
		try {
			t = R._api._PrimaryInfo_Create(), R._api._PrimaryInfo_X_Set(t, this.x), R._api._PrimaryInfo_Y_Set(t, this.y), R._api._PrimaryInfo_Z_Set(t, this.z), e(t);
		} finally {
			R._api._PrimaryInfo_Dispose(t);
		}
	}
}, Je = class {
	channel;
	depth;
	entropy;
	kurtosis;
	maximum;
	mean;
	minimum;
	skewness;
	standardDeviation;
	constructor(e, t) {
		this.channel = e, this.depth = Number(R._api._ChannelStatistics_Depth_Get(t)), this.entropy = R._api._ChannelStatistics_Entropy_Get(t), this.kurtosis = R._api._ChannelStatistics_Kurtosis_Get(t), this.maximum = R._api._ChannelStatistics_Maximum_Get(t), this.mean = R._api._ChannelStatistics_Mean_Get(t), this.minimum = R._api._ChannelStatistics_Minimum_Get(t), this.skewness = R._api._ChannelStatistics_Skewness_Get(t), this.standardDeviation = R._api._ChannelStatistics_StandardDeviation_Get(t);
	}
}, Ye = class e {
	_channels = /* @__PURE__ */ new Map();
	get channels() {
		return Array.from(this._channels.keys());
	}
	composite() {
		return this._channels.get(h.Composite);
	}
	getChannel(e) {
		let t = this._channels.get(e);
		return t === void 0 ? null : t;
	}
	static _create(t, n, r) {
		let i = new e();
		return t.channels.forEach((e) => {
			r >> e & 1 && i.addChannel(n, e);
		}), i.addChannel(n, h.Composite), i;
	}
	addChannel(e, t) {
		let n = R._api._Statistics_GetInstance(e, T(t));
		n !== R._api._NullPointer && this._channels.set(t, new Je(t, n));
	}
}, Xe = class {
	static toArray(e) {
		if (e === R._api._NullPointer) return null;
		let t = Number(R._api._StringInfo_Datum_Get(e)), n = Number(R._api._StringInfo_Length_Get(e));
		return R._api.HEAPU8.subarray(t, t + n);
	}
}, Ze = class {
	constructor(e) {
		this.error = e;
	}
	error;
}, Qe = class t extends xe {
	_settings;
	_progress;
	_warning;
	constructor(e, t) {
		super(e, R._api._MagickImage_Dispose), this._settings = t, this._settings._onArtifact = this.onSettingsArtifactChanged.bind(this);
	}
	get animationDelay() {
		return Number(R._api._MagickImage_AnimationDelay_Get(this._instance));
	}
	set animationDelay(e) {
		R._api._MagickImage_AnimationDelay_Set(this._instance, T(e));
	}
	get animationIterations() {
		return Number(R._api._MagickImage_AnimationIterations_Get(this._instance));
	}
	set animationIterations(e) {
		R._api._MagickImage_AnimationIterations_Set(this._instance, T(e));
	}
	get animationTicksPerSecond() {
		return Number(R._api._MagickImage_AnimationTicksPerSecond_Get(this._instance));
	}
	set animationTicksPerSecond(e) {
		R._api._MagickImage_AnimationTicksPerSecond_Set(this._instance, T(e));
	}
	get artifactNames() {
		let e = [];
		R._api._MagickImage_ResetArtifactIterator(this._instance);
		let t = R._api._MagickImage_GetNextArtifactName(this._instance);
		for (; t !== R._api._NullPointer;) e.push(x(t)), t = R._api._MagickImage_GetNextArtifactName(this._instance);
		return e;
	}
	get attributeNames() {
		let e = [];
		R._api._MagickImage_ResetAttributeIterator(this._instance);
		let t = R._api._MagickImage_GetNextAttributeName(this._instance);
		for (; t !== R._api._NullPointer;) e.push(x(t)), t = R._api._MagickImage_GetNextAttributeName(this._instance);
		return e;
	}
	get backgroundColor() {
		let e = R._api._MagickImage_BackgroundColor_Get(this._instance);
		return w._create(e);
	}
	set backgroundColor(e) {
		e._use((e) => {
			R._api._MagickImage_BackgroundColor_Set(this._instance, e);
		});
	}
	get baseHeight() {
		return Number(R._api._MagickImage_BaseHeight_Get(this._instance));
	}
	get baseWidth() {
		return Number(R._api._MagickImage_BaseWidth_Get(this._instance));
	}
	get blackPointCompensation() {
		return R._api._MagickImage_BlackPointCompensation_Get(this._instance) === 1;
	}
	set blackPointCompensation(e) {
		R._api._MagickImage_BlackPointCompensation_Set(this._instance, +!!e);
	}
	get borderColor() {
		let e = R._api._MagickImage_BorderColor_Get(this._instance);
		return w._create(e);
	}
	set borderColor(e) {
		e._use((e) => {
			R._api._MagickImage_BorderColor_Set(this._instance, e);
		});
	}
	get boundingBox() {
		return this.useExceptionPointer((e) => {
			let t = R._api._MagickImage_BoundingBox_Get(this._instance, e), n = E._fromRectangle(t);
			return n.width === 0 || n.height === 0 ? null : n;
		});
	}
	get channelCount() {
		return Number(R._api._MagickImage_ChannelCount_Get(this._instance));
	}
	get channels() {
		let e = [];
		return [
			h.Red,
			h.Green,
			h.Blue,
			h.Black,
			h.Alpha
		].forEach((t) => {
			R._api._MagickImage_HasChannel(this._instance, T(t)) && e.push(t);
		}), e;
	}
	get chromaticity() {
		return new ee(qe._create(R._api._MagickImage_ChromaRed_Get(this._instance)), qe._create(R._api._MagickImage_ChromaGreen_Get(this._instance)), qe._create(R._api._MagickImage_ChromaBlue_Get(this._instance)), qe._create(R._api._MagickImage_ChromaWhite_Get(this._instance)));
	}
	set chromaticity(e) {
		e.blue._use((e) => R._api._MagickImage_ChromaBlue_Set(this._instance, e)), e.green._use((e) => R._api._MagickImage_ChromaGreen_Set(this._instance, e)), e.red._use((e) => R._api._MagickImage_ChromaRed_Set(this._instance, e)), e.white._use((e) => R._api._MagickImage_ChromaWhite_Set(this._instance, e));
	}
	get classType() {
		return Number(R._api._MagickImage_ClassType_Get(this._instance));
	}
	set classType(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_ClassType_Set(this._instance, T(e), t);
		});
	}
	get colorFuzz() {
		return I._fromQuantum(R._api._MagickImage_ColorFuzz_Get(this._instance));
	}
	set colorFuzz(e) {
		let t = e._toQuantum();
		R._api._MagickImage_ColorFuzz_Set(this._instance, t), this._settings._colorFuzz = t;
	}
	get colormapSize() {
		return Number(R._api._MagickImage_ColormapSize_Get(this._instance));
	}
	set colormapSize(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_ColormapSize_Set(this._instance, T(e), t);
		});
	}
	get colorSpace() {
		return Number(R._api._MagickImage_ColorSpace_Get(this._instance));
	}
	set colorSpace(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_ColorSpace_Set(this._instance, T(e), t);
		});
	}
	get colorType() {
		return this.settings.colorType === void 0 ? Number(R._api._MagickImage_ColorType_Get(this._instance)) : this.settings.colorType;
	}
	set colorType(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_ColorType_Set(this._instance, T(e), t);
		});
	}
	get comment() {
		return this.getAttribute("comment");
	}
	set comment(e) {
		e === null ? this.removeAttribute("comment") : this.setAttribute("comment", e);
	}
	get compose() {
		return Number(R._api._MagickImage_Compose_Get(this._instance));
	}
	set compose(e) {
		R._api._MagickImage_Compose_Set(this._instance, T(e));
	}
	get compression() {
		return Number(R._api._MagickImage_Compression_Get(this._instance));
	}
	get density() {
		return new _e(R._api._MagickImage_ResolutionX_Get(this._instance), R._api._MagickImage_ResolutionY_Get(this._instance), Number(R._api._MagickImage_ResolutionUnits_Get(this._instance)));
	}
	set density(e) {
		R._api._MagickImage_ResolutionX_Set(this._instance, e.x), R._api._MagickImage_ResolutionY_Set(this._instance, e.y), R._api._MagickImage_ResolutionUnits_Set(this._instance, T(e.units));
	}
	get depth() {
		return Number(R._api._MagickImage_Depth_Get(this._instance));
	}
	set depth(e) {
		R._api._MagickImage_Depth_Set(this._instance, T(e));
	}
	get endian() {
		return Number(R._api._MagickImage_Endian_Get(this._instance));
	}
	set endian(e) {
		R._api._MagickImage_Endian_Set(this._instance, T(e));
	}
	get fileName() {
		return b(R._api._MagickImage_FileName_Get(this._instance));
	}
	get filterType() {
		return Number(R._api._MagickImage_FilterType_Get(this._instance));
	}
	set filterType(e) {
		R._api._MagickImage_FilterType_Set(this._instance, T(e));
	}
	get format() {
		return b(R._api._MagickImage_Format_Get(this._instance), "");
	}
	set format(e) {
		C(e.toString(), (e) => R._api._MagickImage_Format_Set(this._instance, e));
	}
	get gamma() {
		return R._api._MagickImage_Gamma_Get(this._instance);
	}
	get gifDisposeMethod() {
		return Number(R._api._MagickImage_GifDisposeMethod_Get(this._instance));
	}
	set gifDisposeMethod(e) {
		R._api._MagickImage_GifDisposeMethod_Set(this._instance, T(e));
	}
	get hasAlpha() {
		return this.toBool(R._api._MagickImage_HasAlpha_Get(this._instance));
	}
	set hasAlpha(e) {
		this.useExceptionPointer((t) => {
			e && this.alpha(m.Opaque), R._api._MagickImage_HasAlpha_Set(this._instance, this.fromBool(e), t);
		});
	}
	get height() {
		return Number(R._api._MagickImage_Height_Get(this._instance));
	}
	get interlace() {
		return Number(R._api._MagickImage_Interlace_Get(this._instance));
	}
	get isOpaque() {
		return this.useExceptionPointer((e) => this.toBool(R._api._MagickImage_IsOpaque_Get(this._instance, e)));
	}
	get interpolate() {
		return Number(R._api._MagickImage_Interpolate_Get(this._instance));
	}
	set interpolate(e) {
		R._api._MagickImage_Interpolate_Set(this._instance, T(e));
	}
	get label() {
		return this.getAttribute("label");
	}
	set label(e) {
		e === null ? this.removeAttribute("label") : this.setAttribute("label", e);
	}
	get matteColor() {
		let e = R._api._MagickImage_MatteColor_Get(this._instance);
		return w._create(e);
	}
	set matteColor(e) {
		e._use((e) => {
			R._api._MagickImage_MatteColor_Set(this._instance, e);
		});
	}
	get metaChannelCount() {
		return Number(R._api._MagickImage_MetaChannelCount_Get(this._instance));
	}
	set metaChannelCount(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_MetaChannelCount_Set(this._instance, T(e), t);
		});
	}
	get orientation() {
		return Number(R._api._MagickImage_Orientation_Get(this._instance));
	}
	set orientation(e) {
		R._api._MagickImage_Orientation_Set(this._instance, T(e));
	}
	get onProgress() {
		return this._progress;
	}
	set onProgress(e) {
		e === void 0 ? this.disposeProgressDelegate() : nt.setProgressDelegate(this), this._progress = e;
	}
	get onWarning() {
		return this._warning;
	}
	set onWarning(e) {
		this._warning = e;
	}
	get page() {
		let e = R._api._MagickImage_Page_Get(this._instance);
		return E._fromRectangle(e);
	}
	set page(e) {
		e._toRectangle((e) => {
			R._api._MagickImage_Page_Set(this._instance, e);
		});
	}
	get profileNames() {
		let e = [];
		R._api._MagickImage_ResetProfileIterator(this._instance);
		let t = R._api._MagickImage_GetNextProfileName(this._instance);
		for (; t !== R._api._NullPointer;) e.push(x(t)), t = R._api._MagickImage_GetNextProfileName(this._instance);
		return e;
	}
	get quality() {
		return Number(R._api._MagickImage_Quality_Get(this._instance));
	}
	set quality(e) {
		let t = e < 1 ? 1 : e;
		t = t > 100 ? 100 : t, R._api._MagickImage_Quality_Set(this._instance, T(t)), this._settings._quality = t;
	}
	get renderingIntent() {
		return Number(R._api._MagickImage_RenderingIntent_Get(this._instance));
	}
	set renderingIntent(e) {
		R._api._MagickImage_RenderingIntent_Set(this._instance, T(e));
	}
	get settings() {
		return this._settings;
	}
	get signature() {
		return this.useExceptionPointer((e) => b(R._api._MagickImage_Signature_Get(this._instance, e)));
	}
	get totalColors() {
		return this.useExceptionPointer((e) => Number(R._api._MagickImage_TotalColors_Get(this._instance, e)));
	}
	get virtualPixelMethod() {
		return Number(R._api._MagickImage_VirtualPixelMethod_Get(this._instance));
	}
	set virtualPixelMethod(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_VirtualPixelMethod_Set(this._instance, T(e), t);
		});
	}
	get width() {
		return Number(R._api._MagickImage_Width_Get(this._instance));
	}
	adaptiveBlur(e, t) {
		let n = this.valueOrDefault(e, 0), r = this.valueOrDefault(t, 1);
		this.useException((e) => {
			let t = R._api._MagickImage_AdaptiveBlur(this._instance, n, r, e.ptr);
			this._setInstance(t, e);
		});
	}
	adaptiveResize(e, t) {
		let n = typeof e == "number" ? new E(0, 0, e, t) : e;
		this.useException((e) => {
			C(n.toString(), (t) => {
				let n = R._api._MagickImage_AdaptiveResize(this._instance, t, e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	adaptiveSharpen(e, t, n) {
		let r = 0, i = t ?? 1, a = n ?? g.Undefined;
		e !== void 0 && (t === void 0 ? a = e : r = e), this.useException((e) => {
			let t = R._api._MagickImage_AdaptiveSharpen(this._instance, r, i, T(a), e.ptr);
			this._setInstance(t, e);
		});
	}
	adaptiveThreshold(e, t, n, r) {
		let i = n instanceof I ? n._toQuantum() : 0, a = r ?? g.Undefined;
		typeof n == "number" && (a = n), this.useException((n) => {
			let r = R._api._MagickImage_AdaptiveThreshold(this._instance, T(e), T(t), i, T(a), n.ptr);
			this._setInstance(r, n);
		});
	}
	addNoise(e, t, n) {
		let r = 1, i = n ?? g.Undefined;
		t !== void 0 && (n === void 0 ? i = t : r = t), this.useException((t) => {
			let n = R._api._MagickImage_AddNoise(this._instance, T(e), r, T(i), t.ptr);
			this._setInstance(n, t);
		});
	}
	affineTransform(e) {
		this.useException((t) => {
			let n = R._api._MagickImage_AffineTransform(this._instance, e.scaleX, e.scaleY, e.shearX, e.shearY, e.translateX, e.translateY, t.ptr);
			this._setInstance(n, t);
		});
	}
	alpha(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_SetAlpha(this._instance, T(e), t);
		});
	}
	annotate(e, t, n, r) {
		return this.useExceptionPointer((i) => this._settings._drawing._use((a) => {
			C(e, (e) => {
				let o = null, s = j.Undefined, c = 0;
				typeof t == "object" ? (o = t.toString(), n !== void 0 && (s = n), r !== void 0 && (c = r)) : (s = t, n !== void 0 && (c = n)), C(o, (t) => {
					R._api._MagickImage_Annotate(this._instance, a._instance, e, t, T(s), c, i);
				});
			});
		}));
	}
	autoGamma(e) {
		this.useExceptionPointer((t) => {
			let n = this.valueOrDefault(e, g.Composite);
			R._api._MagickImage_AutoGamma(this._instance, T(n), t);
		});
	}
	autoLevel(e) {
		this.useExceptionPointer((t) => {
			let n = this.valueOrDefault(e, g.Undefined);
			R._api._MagickImage_AutoLevel(this._instance, T(n), t);
		});
	}
	autoOrient() {
		this.useException((e) => {
			let t = R._api._MagickImage_AutoOrient(this._instance, e.ptr);
			this._setInstance(t, e);
		});
	}
	autoThreshold(e) {
		this.useException((t) => {
			R._api._MagickImage_AutoThreshold(this._instance, T(e), t.ptr);
		});
	}
	bilateralBlur(e, t, n, r) {
		let i = this.valueOrComputedDefault(n, () => Math.sqrt(Number(e * e + t * t))), a = this.valueOrDefault(r, i * .25);
		this.useException((n) => {
			let r = R._api._MagickImage_BilateralBlur(this._instance, T(e), T(t), i, a, n.ptr);
			this._setInstance(r, n);
		});
	}
	blackThreshold(e, t) {
		let n = this.valueOrDefault(t, g.Composite);
		this.useException((t) => {
			C(e.toString(), (e) => {
				R._api._MagickImage_BlackThreshold(this._instance, e, T(n), t.ptr);
			});
		});
	}
	blueShift(e) {
		let t = this.valueOrDefault(e, 1.5);
		this.useException((e) => {
			let n = R._api._MagickImage_BlueShift(this._instance, t, e.ptr);
			this._setInstance(n, e);
		});
	}
	blur(e, t, n) {
		let r = 0, i = this.valueOrDefault(t, 1), a = this.valueOrDefault(n, g.Undefined);
		e !== void 0 && (t === void 0 ? a = e : r = e), this.useException((e) => {
			let t = R._api._MagickImage_Blur(this._instance, r, i, T(a), e.ptr);
			this._setInstance(t, e);
		});
	}
	border(e, t) {
		let n = new E(0, 0, e, this.valueOrDefault(t, e));
		this.useException((e) => {
			n._toRectangle((t) => {
				let n = R._api._MagickImage_Border(this._instance, t, e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	brightnessContrast(e, t, n) {
		let r = this.valueOrDefault(n, g.Undefined);
		this.useException((n) => {
			R._api._MagickImage_BrightnessContrast(this._instance, e.toDouble(), t.toDouble(), T(r), n.ptr);
		});
	}
	cannyEdge(e, t, n, r) {
		let i = this.valueOrDefault(e, 0), a = this.valueOrDefault(t, 1), o = this.valueOrDefault(n, new I(10)).toDouble() / 100, s = this.valueOrDefault(r, new I(30)).toDouble() / 100;
		this.useException((e) => {
			let t = R._api._MagickImage_CannyEdge(this._instance, i, a, o, s, e.ptr);
			this._setInstance(t, e);
		});
	}
	charcoal(e, t) {
		let n = e === void 0 ? 0 : e, r = t === void 0 ? 1 : t;
		this.useException((e) => {
			let t = R._api._MagickImage_Charcoal(this._instance, n, r, e.ptr);
			this._setInstance(t, e);
		});
	}
	chop(e) {
		this.useException((t) => {
			e._toRectangle((e) => {
				let n = R._api._MagickImage_Chop(this._instance, e, t.ptr);
				this._setInstance(n, t);
			});
		});
	}
	chopHorizontal(e, t) {
		this.chop(new E(e, 0, t, 0));
	}
	chopVertical(e, t) {
		this.chop(new E(0, e, 0, t));
	}
	clahe(e, t, n, r) {
		this.useExceptionPointer((i) => {
			let a = T(e instanceof I ? e.multiply(this.width) : e), o = T(t instanceof I ? t.multiply(this.height) : t);
			R._api._MagickImage_Clahe(this._instance, a, o, T(n), r, i);
		});
	}
	clone(e) {
		return t._clone(this)._use(e);
	}
	cloneArea(e, n) {
		return k.usePointer((r) => e._toRectangle((i) => Be._use(0, 0, (a) => {
			let o = R._api._MagickImage_CloneArea(this._instance, T(e.width), T(e.height), r);
			return R._api._MagickImage_CopyPixels(o, this._instance, i, a, T(g.Undefined), r), n(new t(o, this._settings));
		})));
	}
	clut(e, t, n) {
		let r = this.valueOrDefault(t, Ke.Undefined), i = this.valueOrDefault(n, g.Undefined);
		this.useExceptionPointer((t) => {
			R._api._MagickImage_Clut(this._instance, e._instance, T(r), T(i), t);
		});
	}
	colorAlpha(e) {
		if (!this.hasAlpha) return;
		let n = t.create();
		n.read(e, this.width, this.height), n.composite(this, le.SrcOver, new me(0, 0)), this._instance = n._instance;
	}
	colorDecisionList(e) {
		this.useExceptionPointer((t) => {
			C(e, (e) => {
				R._api._MagickImage_ColorDecisionList(this._instance, e, t);
			});
		});
	}
	compare(e, n, r, i) {
		let a = n instanceof ce, o = a ? n.metric : n, s = r;
		i !== void 0 && (s = i);
		let c = g.Undefined;
		if (typeof s != "function") return s !== void 0 && (c = s), this.useExceptionPointer((t) => R._api._MagickImage_CompareDistortion(this._instance, e._instance, T(o), T(c), t));
		r !== void 0 && typeof r != "function" && (c = r);
		let l = Re.use(this, (r) => (a && n._setArtifacts(r), A.use((n) => {
			let r = this.useExceptionPointer((t) => R._api._MagickImage_Compare(this._instance, e._instance, T(o), T(c), n.ptr, t)), i = n.value, a = t._createFromImage(r, this._settings);
			return se._create(i, a);
		})));
		return l.difference._use(() => s(l));
	}
	composite(e, t, n, r, i) {
		let a = 0, o = 0, s = le.In, c = g.All, l = null;
		t instanceof me ? (a = t.x, o = t.y) : t !== void 0 && (s = t), n instanceof me ? (a = n.x, o = n.y) : typeof n == "string" ? l = n : n !== void 0 && (c = n), typeof r == "string" ? l = r : r !== void 0 && (c = r), i !== void 0 && (c = i), l !== null && this.setArtifact("compose:args", l), this.useExceptionPointer((t) => {
			R._api._MagickImage_Composite(this._instance, e._instance, T(a), T(o), T(s), T(c), t);
		}), l !== null && this.removeArtifact("compose:args");
	}
	compositeGravity(e, t, n, r, i, a) {
		let o = 0, s = 0, c = le.In, l = g.All, u = null;
		n instanceof me ? (o = n.x, s = n.y) : n !== void 0 && (c = n), r instanceof me ? (o = r.x, s = r.y) : typeof r == "string" ? u = r : r !== void 0 && (l = r), typeof i == "string" ? u = i : i !== void 0 && (l = i), a !== void 0 && (l = a), u !== null && this.setArtifact("compose:args", u), this.useExceptionPointer((n) => {
			R._api._MagickImage_CompositeGravity(this._instance, e._instance, T(t), T(o), T(s), T(c), T(l), n);
		}), u !== null && this.removeArtifact("compose:args");
	}
	connectedComponents(e) {
		let t = typeof e == "number" ? new ge(e) : e;
		return Re.use(this, (e) => (t._setArtifacts(e), this.useException((e) => be.use((n) => {
			try {
				let r = R._api._MagickImage_ConnectedComponents(this._instance, T(t.connectivity), n.ptr, e.ptr);
				return this._setInstance(r, e), he._create(n.value, Number(this.colormapSize));
			} finally {
				n.value !== R._api._NullPointer && R._api._ConnectedComponent_DisposeList(n.value);
			}
		}))));
	}
	contrast = () => this.contrastPrivate(!0);
	contrastStretch(e, t, n) {
		let r = Number(this.width * this.height), i = e.multiply(r), a = 0, o = this.valueOrDefault(n, g.Undefined);
		t instanceof I ? a = r - t.multiply(r) : (a = r - e.multiply(r), t !== void 0 && (o = t)), this.useExceptionPointer((e) => {
			R._api._MagickImage_ContrastStretch(this._instance, i, a, T(o), e);
		});
	}
	static create(e, n, r) {
		let i = new t(t.createInstance(), new Fe());
		return e !== void 0 && i.readOrPing(!1, e, n, r), i;
	}
	crop(e, t, n) {
		let r, i;
		typeof e == "number" ? t !== void 0 && (r = new E(e, t), i = this.valueOrDefault(n, j.Undefined)) : (r = e, i = this.valueOrDefault(t, j.Undefined)), this.useException((e) => {
			C(r.toString(), (t) => {
				let n = R._api._MagickImage_Crop(this._instance, t, T(i), e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	cropToTiles(e, t, n) {
		let r, i;
		return typeof e == "number" && typeof t == "number" && n !== void 0 ? (r = new E(0, 0, e, t), i = n) : typeof e != "number" && typeof t != "number" && (r = e, i = t), this.useException((e) => C(r.toString(), (t) => {
			let n = R._api._MagickImage_CropToTiles(this._instance, t, e.ptr);
			return F._createFromImages(n, this._settings, (e) => i(e));
		}));
	}
	cycleColormap(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_CycleColormap(this._instance, T(e), t);
		});
	}
	deskew(e, t) {
		return Re.use(this, (n) => {
			t !== void 0 && n.setArtifact("deskew:auto-crop", t), this.useException((t) => {
				let n = R._api._MagickImage_Deskew(this._instance, e._toQuantum(), t.ptr);
				this._setInstance(n, t);
			});
			let r = Number(this.getArtifact("deskew:angle"));
			return isNaN(r) ? 0 : r;
		});
	}
	determineBitDepth(e) {
		let t = this.valueOrDefault(e, g.Undefined);
		return this.useExceptionPointer((e) => Number(R._api._MagickImage_DetermineBitDepth(this._instance, T(t), e)));
	}
	distort(e, t) {
		Re.use(this, (n) => {
			let r, i = 0;
			typeof e == "number" ? r = e : (r = e.method, i = +!!e.bestFit, e._setArtifacts(n)), this.useException((e) => {
				we(t, (n) => {
					let a = R._api._MagickImage_Distort(this._instance, T(r), i, n, T(t.length), e.ptr);
					this._setInstance(a, e);
				});
			});
		});
	}
	draw(...e) {
		let t = e.flat();
		t.length !== 0 && Ee._use(this, (e) => {
			e.draw(t);
		});
	}
	evaluate(e, t, n, r) {
		if (typeof t == "number") {
			let r = t, i = typeof n == "number" ? n : n._toQuantum();
			this.useExceptionPointer((t) => {
				R._api._MagickImage_EvaluateOperator(this._instance, T(e), T(r), i, t);
			});
		} else if (r !== void 0) {
			if (typeof n != "number") throw new y("this should not happen");
			let i = t, a = n, o = typeof r == "number" ? r : r._toQuantum();
			if (i.isPercentage) throw new y("percentage is not supported");
			this.useExceptionPointer((t) => {
				ze.use(this, i, (n) => {
					R._api._MagickImage_EvaluateGeometry(this._instance, T(e), n, T(a), o, t);
				});
			});
		}
	}
	extent(e, t, n) {
		let r = j.Undefined, i;
		typeof e == "number" ? typeof t == "number" && (i = new E(e, t)) : i = e, typeof t == "number" ? r = t : t !== void 0 && (this.backgroundColor = t), typeof n == "number" ? r = n : n !== void 0 && (this.backgroundColor = n), this.useException((e) => {
			C(i.toString(), (t) => {
				let n = R._api._MagickImage_Extent(this._instance, t, T(r), e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	flip() {
		this.useException((e) => {
			let t = R._api._MagickImage_Flip(this._instance, e.ptr);
			this._setInstance(t, e);
		});
	}
	floodFill(e, t, n, r) {
		this.floodFillPrivate(e, t, n, r, !1);
	}
	flop() {
		this.useException((e) => {
			let t = R._api._MagickImage_Flop(this._instance, e.ptr);
			this._setInstance(t, e);
		});
	}
	formatExpression(e) {
		return this.useExceptionPointer((t) => this._settings._use((n) => C(e, (e) => {
			let r = R._api._MagickImage_FormatExpression(this._instance, n._instance, e, t);
			return S(R._api, r);
		})));
	}
	gammaCorrect(e, t) {
		let n = this.valueOrDefault(t, g.Undefined);
		this.useExceptionPointer((t) => {
			R._api._MagickImage_GammaCorrect(this._instance, e, T(n), t);
		});
	}
	gaussianBlur(e, t, n) {
		let r = this.valueOrDefault(t, 1), i = this.valueOrDefault(n, g.Undefined);
		this.useException((t) => {
			let n = R._api._MagickImage_GaussianBlur(this._instance, e, r, T(i), t.ptr);
			this._setInstance(n, t);
		});
	}
	getArtifact(e) {
		return C(e, (e) => b(R._api._MagickImage_GetArtifact(this._instance, e)));
	}
	getAttribute(e) {
		return this.useException((t) => C(e, (e) => b(R._api._MagickImage_GetAttribute(this._instance, e, t.ptr))));
	}
	getColormapColor(e) {
		let t = R._api._MagickImage_GetColormapColor(this._instance, T(e));
		return t === R._api._NullPointer ? null : w._create(t);
	}
	getColorProfile() {
		for (let e of ["icc", "icm"]) {
			let t = this.getProfilePrivate(e);
			if (t !== null) return new ae(t);
		}
		return null;
	}
	getPixels(e) {
		if (this._settings._ping) throw new y("image contains no pixel data");
		return We._use(this, e);
	}
	getProfile(e) {
		let t = this.getProfilePrivate(e);
		return t === null ? null : new ie(e, t);
	}
	getWriteMask(e) {
		let n = this.useExceptionPointer((e) => R._api._MagickImage_GetWriteMask(this._instance, e)), r = n === R._api._NullPointer ? null : new t(n, new Fe());
		return r == null ? e(r) : r._use(e);
	}
	grayscale(e = Ge.Undefined) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_Grayscale(this._instance, T(e), t);
		});
	}
	hasProfile(e) {
		return C(e, (e) => this.toBool(R._api._MagickImage_HasProfile(this._instance, e)));
	}
	histogram() {
		let e = /* @__PURE__ */ new Map();
		return this.useExceptionPointer((t) => {
			be.use((n) => {
				let r = R._api._MagickImage_Histogram(this._instance, n.ptr, t);
				if (r !== R._api._NullPointer) {
					let t = n.value;
					for (let n = 0; n < t; n++) {
						let t = R._api._MagickColorCollection_Get(r, T(n)), i = w._create(t), a = R._api._MagickColor_Count_Get(t);
						e.set(i.toString(), a);
					}
					R._api._MagickColorCollection_Dispose(r);
				}
			});
		}), e;
	}
	inverseContrast = () => this.contrastPrivate(!1);
	inverseFloodFill(e, t, n, r) {
		this.floodFillPrivate(e, t, n, r, !0);
	}
	inverseLevel(e, t, n, r) {
		let i = this.valueOrDefault(n, 1), a = this.valueOrDefault(r, g.Composite);
		this.useExceptionPointer((n) => {
			R._api._MagickImage_InverseLevel(this._instance, e.toDouble(), t._toQuantum(), i, T(a), n);
		});
	}
	inverseLevelColors(e, t, n) {
		this.levelColorsPrivate(!0, e, t, n);
	}
	inverseOpaque = (e, t) => this.opaquePrivate(e, t, !0);
	inverseSigmoidalContrast(e, t, n) {
		this.sigmoidalContrastPrivate(!1, e, t, n);
	}
	inverseTransparent = (e) => this.transparentPrivate(e, !0);
	level(e, t, n, r) {
		let i = this.valueOrDefault(n, 1), a = this.valueOrDefault(r, g.Composite);
		this.useExceptionPointer((n) => {
			R._api._MagickImage_Level(this._instance, e.toDouble(), t._toQuantum(), i, T(a), n);
		});
	}
	levelColors(e, t, n) {
		this.levelColorsPrivate(!1, e, t, n);
	}
	linearStretch(e, t) {
		this.useExceptionPointer((n) => {
			R._api._MagickImage_LinearStretch(this._instance, e.toDouble(), t._toQuantum(), n);
		});
	}
	liquidRescale(e, t) {
		let n = typeof e == "number" ? new E(e, t) : e;
		this.useException((e) => {
			C(n.toString(), (t) => {
				let r = R._api._MagickImage_LiquidRescale(this._instance, t, Number(n.x), Number(n.y), e.ptr);
				this._setInstance(r, e);
			});
		});
	}
	negate(e) {
		this.useExceptionPointer((t) => {
			let n = this.valueOrDefault(e, g.Undefined);
			R._api._MagickImage_Negate(this._instance, 0, T(n), t);
		});
	}
	negateGrayScale(e) {
		this.useExceptionPointer((t) => {
			let n = this.valueOrDefault(e, g.Undefined);
			R._api._MagickImage_Negate(this._instance, 1, T(n), t);
		});
	}
	normalize() {
		this.useExceptionPointer((e) => {
			R._api._MagickImage_Normalize(this._instance, e);
		});
	}
	modulate(e, t, n) {
		let r = this.valueOrDefault(t, new I(100)), i = this.valueOrDefault(n, new I(100));
		this.useExceptionPointer((t) => {
			C(`${e.toDouble()}/${r.toDouble()}/${i.toDouble()}`, (e) => {
				R._api._MagickImage_Modulate(this._instance, e, t);
			});
		});
	}
	morphology(e) {
		this.useException((t) => {
			C(e.kernel, (n) => {
				let r = R._api._MagickImage_Morphology(this._instance, T(e.method), n, T(e.channels), T(e.iterations), t.ptr);
				this._setInstance(r, t);
			});
		});
	}
	motionBlur(e, t, n) {
		this.useException((r) => {
			let i = R._api._MagickImage_MotionBlur(this._instance, e, t, n, r.ptr);
			this._setInstance(i, r);
		});
	}
	oilPaint(e) {
		let t = this.valueOrDefault(e, 3);
		this.useException((e) => {
			let n = R._api._MagickImage_OilPaint(this._instance, t, 0, e.ptr);
			this._setInstance(n, e);
		});
	}
	opaque = (e, t) => this.opaquePrivate(e, t, !1);
	ping(e, t) {
		this.readOrPing(!0, e, t);
	}
	perceptualHash(e) {
		let t = this.valueOrDefault(e, Ue._defaultColorspaces());
		return Ue._validateColorSpaces(t), Re.use(this, (e) => {
			let n = t.map((e) => te[e]).join(",");
			return e.setArtifact("phash:colorspaces", n), this.useExceptionPointer((e) => {
				let n = R._api._MagickImage_PerceptualHash(this._instance, e);
				try {
					return Ue._create(this, t, n);
				} finally {
					R._api._PerceptualHash_DisposeList(n);
				}
			});
		});
	}
	quantize(e) {
		let t = this.valueOrDefault(e, new Le());
		return this.useException((e) => {
			t._use((t) => {
				R._api._MagickImage_Quantize(this._instance, t._instance, e.ptr);
			});
		}), t.measureErrors ? ke._create(this) : null;
	}
	read(e, t, n) {
		this.readOrPing(!1, e, t, n);
	}
	readFromCanvas(e, t) {
		let n = e.getContext("2d", t);
		if (n === null) return;
		let r = n.getImageData(0, 0, e.width, e.height), i = new N();
		i.format = Ae.Rgba, i.width = e.width, i.height = e.height, this.useException((e) => {
			this.readFromArray(r.data, i, e);
		});
	}
	removeArtifact(e) {
		C(e, (e) => {
			R._api._MagickImage_RemoveArtifact(this._instance, e);
		});
	}
	removeAttribute(e) {
		C(e, (e) => {
			R._api._MagickImage_RemoveAttribute(this._instance, e);
		});
	}
	removeProfile(e) {
		C(typeof e == "string" ? e : e.name, (e) => {
			R._api._MagickImage_RemoveProfile(this._instance, e);
		});
	}
	removeWriteMask() {
		this.useExceptionPointer((e) => {
			R._api._MagickImage_SetWriteMask(this._instance, R._api._NullPointer, e);
		});
	}
	resetPage() {
		this.page = new E(0, 0, 0, 0);
	}
	resize(e, t, n) {
		let r = this.filterType, i;
		typeof e == "number" ? (i = new E(e, t), n !== void 0 && (r = n)) : (i = e, t !== void 0 && (r = t)), this.useException((e) => {
			C(i.toString(), (t) => {
				let n = R._api._MagickImage_Resize(this._instance, t, r, e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	roll(e, t) {
		this.useException((n) => {
			let r = R._api._MagickImage_Roll(this._instance, T(e), T(t), n.ptr);
			this._setInstance(r, n);
		});
	}
	rotate(e) {
		this.useException((t) => {
			let n = R._api._MagickImage_Rotate(this._instance, e, t.ptr);
			this._setInstance(n, t);
		});
	}
	separate(e, t) {
		return this.useException((n) => {
			let r, i = g.Undefined;
			if (typeof e == "number" && t !== void 0) i = e, r = t;
			else if (typeof e == "function") r = e;
			else throw new y("invalid arguments");
			let a = R._api._MagickImage_Separate(this._instance, T(i), n.ptr);
			return F._createFromImages(a, this._settings, (e) => r(e));
		});
	}
	sepiaTone(e = new I(80)) {
		this.useException((t) => {
			let n = typeof e == "number" ? new I(e) : e, r = R._api._MagickImage_SepiaTone(this._instance, n._toQuantum(), t.ptr);
			this._setInstance(r, t);
		});
	}
	setArtifact(e, t) {
		let n;
		n = typeof t == "string" ? t : typeof t == "boolean" ? this.fromBool(t).toString() : t.toString(), C(e, (e) => {
			C(n, (t) => {
				R._api._MagickImage_SetArtifact(this._instance, e, t);
			});
		});
	}
	setAttribute(e, t) {
		this.useException((n) => {
			C(e, (e) => {
				C(t, (t) => {
					R._api._MagickImage_SetAttribute(this._instance, e, t, n.ptr);
				});
			});
		});
	}
	setCompression(e) {
		R._api._MagickImage_Compression_Set(this._instance, T(e));
	}
	setProfile(e, t) {
		let n = typeof e == "string" ? e : e.name, r;
		t === void 0 ? typeof e != "string" && (r = e.data) : r = t, this.useException((e) => {
			C(n, (t) => {
				Ce(r, (n) => {
					R._api._MagickImage_SetProfile(this._instance, t, n, T(r.byteLength), e.ptr);
				});
			});
		});
	}
	setWriteMask(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_SetWriteMask(this._instance, e._instance, t);
		});
	}
	shadow(e, t, n, r, i) {
		let a, o = typeof e == "number" ? e : 5, s = this.valueOrDefault(t, 5), c = this.valueOrDefault(n, .5), l = this.valueOrDefault(r, new I(80)), u = typeof e == "number" ? i : e;
		u !== void 0 && (a = this.backgroundColor, this.backgroundColor = u);
		try {
			this.useException((e) => {
				let t = R._api._MagickImage_Shadow(this._instance, T(o), T(s), c, l.toDouble(), e.ptr);
				this._setInstance(t, e);
			});
		} finally {
			a !== void 0 && (this.backgroundColor = a);
		}
	}
	sharpen(e, t, n) {
		let r = this.valueOrDefault(e, 0), i = this.valueOrDefault(t, 1), a = this.valueOrDefault(n, g.Undefined);
		this.useException((e) => {
			let t = R._api._MagickImage_Sharpen(this._instance, r, i, T(a), e.ptr);
			this._setInstance(t, e);
		});
	}
	shave(e, t) {
		this.useException((n) => {
			let r = R._api._MagickImage_Shave(this._instance, T(e), T(t), n.ptr);
			this._setInstance(r, n);
		});
	}
	sigmoidalContrast(e, t, n) {
		this.sigmoidalContrastPrivate(!0, e, t, n);
	}
	solarize(e = new I(50)) {
		this.useException((t) => {
			let n = typeof e == "number" ? new I(e) : e;
			R._api._MagickImage_Solarize(this._instance, n._toQuantum(), t.ptr);
		});
	}
	splice(e, t) {
		let n = this.valueOrDefault(t, j.Undefined);
		C(e.toString(), (e) => {
			this.useException((t) => {
				let r = R._api._MagickImage_Splice(this._instance, e, T(n), t.ptr);
				this._setInstance(r, t);
			});
		});
	}
	statistics(e) {
		let t = this.valueOrDefault(e, g.All);
		return this.useExceptionPointer((e) => {
			let n = R._api._MagickImage_Statistics(this._instance, T(t), e), r = Ye._create(this, n, t);
			return R._api._Statistics_DisposeList(n), r;
		});
	}
	strip() {
		this.useExceptionPointer((e) => {
			R._api._MagickImage_Strip(this._instance, e);
		});
	}
	transformColorSpace(e, t, n) {
		let r = e, i, a = oe.Quantum;
		t !== void 0 && (typeof t == "number" ? a = t : i = t), n !== void 0 && (a = n);
		let o = this.hasProfile("icc") || this.hasProfile("icm");
		if (i === void 0) {
			if (!o) return !1;
			i = r;
		} else {
			if (r.colorSpace !== this.colorSpace) return !1;
			o || this.setProfile(r);
		}
		return a === oe.Quantum ? Re.use(this, (e) => {
			e.setArtifact("profile:highres-transform", !1), this.setProfile(i);
		}) : this.setProfile(i), !0;
	}
	threshold(e, t) {
		let n = this.valueOrDefault(t, g.Undefined);
		this.useExceptionPointer((t) => {
			R._api._MagickImage_Threshold(this._instance, e._toQuantum(), T(n), t);
		});
	}
	thumbnail(e, t) {
		let n = typeof e == "number" ? new E(e, t) : e;
		this.useException((e) => {
			C(n.toString(), (t) => {
				let n = R._api._MagickImage_Thumbnail(this._instance, t, e.ptr);
				this._setInstance(n, e);
			});
		});
	}
	toString = () => `${this.format} ${this.width}x${this.height} ${this.depth}-bit ${te[this.colorSpace]}`;
	transparent(e) {
		e._use((e) => {
			this.useExceptionPointer((t) => {
				R._api._MagickImage_Transparent(this._instance, e, 0, t);
			});
		});
	}
	trim(...e) {
		if (e.length > 0) {
			if (e.length == 1 && e[0] instanceof I) {
				let t = e[0];
				this.setArtifact("trim:percent-background", t.toDouble().toString());
			} else {
				let t = [...new Set(De(e))].join(",");
				this.setArtifact("trim:edges", t);
			}
		}
		this.useException((e) => {
			let t = R._api._MagickImage_Trim(this._instance, e.ptr);
			this._setInstance(t, e), this.removeArtifact("trim:edges"), this.removeArtifact("trim:percent-background");
		});
	}
	wave(e, t, n) {
		let r = this.valueOrDefault(e, this.interpolate), i = this.valueOrDefault(t, 25), a = this.valueOrDefault(n, 150);
		this.useException((e) => {
			let t = R._api._MagickImage_Wave(this._instance, T(r), i, a, e.ptr);
			this._setInstance(t, e);
		});
	}
	vignette(e, t, n, r) {
		let i = this.valueOrDefault(e, 0), a = this.valueOrDefault(t, 1), o = this.valueOrDefault(n, 0), s = this.valueOrDefault(r, 0);
		this.useException((e) => {
			let t = R._api._MagickImage_Vignette(this._instance, i, a, T(o), T(s), e.ptr);
			this._setInstance(t, e);
		});
	}
	whiteThreshold(e, t) {
		let n = this.valueOrDefault(t, g.Composite);
		this.useException((t) => {
			C(e.toString(), (e) => {
				R._api._MagickImage_WhiteThreshold(this._instance, e, T(n), t.ptr);
			});
		});
	}
	write(e, t) {
		let n = R._api._NullPointer, r = 0;
		t === void 0 ? t = e : this._settings.format = e, this.useException((e) => {
			be.use((t) => {
				this._settings._use((i) => {
					try {
						n = R._api._MagickImage_WriteBlob(this._instance, i._instance, t.ptr, e.ptr), r = Number(t.value);
					} catch (e) {
						throw n !== R._api._NullPointer && (n = R._api._MagickMemory_Relinquish(n)), e;
					}
				});
			});
		});
		let i = new ve(n, r, t);
		return O._disposeAfterExecution(i, i.func);
	}
	writeToCanvas(e, t) {
		e.width = this.width, e.height = this.height;
		let n = e.getContext("2d", t);
		n !== null && We._map(this, "RGBA", (e) => {
			let t = n.createImageData(this.width, this.height), r = 0;
			for (let n = 0; n < this.height; n++) for (let n = 0; n < this.width; n++) t.data[r++] = R._api.HEAPU8[e++], t.data[r++] = R._api.HEAPU8[e++], t.data[r++] = R._api.HEAPU8[e++], t.data[r++] = R._api.HEAPU8[e++];
			n.putImageData(t, 0, 0);
		});
	}
	static _createFromImage(e, n) {
		return new t(e, n);
	}
	_channelOffset(e) {
		return R._api._MagickImage_HasChannel(this._instance, T(e)) ? Number(R._api._MagickImage_ChannelOffset(this._instance, T(e))) : -1;
	}
	static _clone(e) {
		return k.usePointer((n) => new t(R._api._MagickImage_Clone(e._instance, n), e._settings._clone()));
	}
	_getSettings() {
		return this._settings;
	}
	_instanceNotInitialized() {
		throw new y("no image has been read");
	}
	_setInstance(e, t) {
		if (super._setInstance(e, t) === !0 || e === R._api._NullPointer && this.onProgress !== void 0) return !0;
		throw new y("out of memory");
	}
	_use(e) {
		return O._disposeAfterExecution(this, e);
	}
	static _create(e) {
		return t.create()._use(e);
	}
	onDispose() {
		this.disposeProgressDelegate();
	}
	contrastPrivate(e) {
		this.useExceptionPointer((t) => {
			R._api._MagickImage_Contrast(this._instance, this.fromBool(e), t);
		});
	}
	static createInstance() {
		return k.usePointer((e) => R._api._MagickImage_Create(R._api._NullPointer, e));
	}
	disposeProgressDelegate() {
		nt.removeProgressDelegate(this), this._progress = void 0;
	}
	floodFillPrivate(e, n, r, i, a) {
		let o = i;
		o === void 0 && this.getPixels((e) => {
			let t = e.getColor(n, r);
			t !== null && (o = t);
		}), typeof e == "number" && o !== void 0 && (o.a = e), this.settings._drawing._use((i) => {
			e instanceof w ? (i.setFillColor(e), i.setFillPattern()) : e instanceof t && (i.setFillColor(), i.setFillPattern(e)), this.useExceptionPointer((e) => {
				o === void 0 ? R._api._MagickImage_FloodFill(this._instance, i._instance, T(n), T(r), R._api._NullPointer, this.fromBool(a), e) : o._use((t) => {
					R._api._MagickImage_FloodFill(this._instance, i._instance, T(n), T(r), t, this.fromBool(a), e);
				});
			});
		});
	}
	fromBool(e) {
		return +!!e;
	}
	getProfilePrivate(e) {
		return C(e, (e) => {
			let t = R._api._MagickImage_GetProfile(this._instance, e), n = Xe.toArray(t);
			return n === null ? null : n;
		});
	}
	levelColorsPrivate(e, t, n, r) {
		let i = this.valueOrDefault(r, g.RGB);
		this.useExceptionPointer((r) => {
			t._use((t) => {
				n._use((n) => {
					R._api._MagickImage_LevelColors(this._instance, t, n, T(i), this.fromBool(e), r);
				});
			});
		});
	}
	onSettingsArtifactChanged(e, t) {
		t === void 0 ? this.removeArtifact(e) : this.setArtifact(e, t);
	}
	opaquePrivate(e, t, n) {
		this.useExceptionPointer((r) => {
			e._use((e) => {
				t._use((t) => {
					R._api._MagickImage_Opaque(this._instance, e, t, this.fromBool(n), r);
				});
			});
		});
	}
	readOrPing(t, n, r, i) {
		this.useException((a) => {
			let o = r instanceof N ? r : new N(this._settings);
			if (o._ping = t, this._settings._ping = t, o.frameCount !== void 0 && o.frameCount > 1) throw new y("The frame count can only be set to 1 when a single image is being read.");
			if (typeof n == "string") o._fileName = n;
			else if (e(n)) {
				this.readFromArray(n, o, a);
				return;
			} else o._fileName = "xc:" + n.toShortString(), o.width = typeof r == "number" ? r : 0, o.height = typeof i == "number" ? i : 0;
			o._use((e) => {
				let t = R._api._MagickImage_ReadFile(e._instance, a.ptr);
				this._setInstance(t, a);
			});
		});
	}
	readFromArray(e, t, n) {
		t._use((t) => {
			Ce(e, (r) => {
				let i = T(e.byteLength), a = R._api._MagickImage_ReadBlob(t._instance, r, R._api._NullPointer, i, n.ptr);
				this._setInstance(a, n);
			});
		});
	}
	sigmoidalContrastPrivate(e, t, n, r) {
		let i;
		i = n === void 0 ? de.max * .5 : typeof n == "number" ? n : n.multiply(de.max);
		let a = T(this.valueOrDefault(r, g.Undefined));
		this.useExceptionPointer((n) => {
			R._api._MagickImage_SigmoidalContrast(this._instance, this.fromBool(e), t, i, a, n);
		});
	}
	toBool(e) {
		return e === 1;
	}
	transparentPrivate(e, t) {
		e._use((e) => {
			this.useExceptionPointer((n) => {
				R._api._MagickImage_Transparent(this._instance, e, this.fromBool(t), n);
			});
		});
	}
	valueOrDefault(e, t) {
		return e === void 0 ? t : e;
	}
	valueOrComputedDefault(e, t) {
		return e === void 0 ? t() : e;
	}
	useException(e) {
		return k.use(e, (e) => {
			this.onWarning !== void 0 && this.onWarning(new Ze(e));
		});
	}
	useExceptionPointer(e) {
		return k.usePointer(e, (e) => {
			this.onWarning !== void 0 && this.onWarning(new Ze(e));
		});
	}
};
//#endregion
//#region node_modules/@dlemstra/magick-native/x64/magick.js
async function $e(e = {}) {
	var t = e, n = !!globalThis.window, r = !!globalThis.WorkerGlobalScope;
	globalThis.process?.versions?.node && globalThis.process?.type, (!globalThis.crypto || !globalThis.crypto.getRandomValues) && (globalThis.crypto = { getRandomValues: (e) => {
		for (let t = 0; t < e.length; t++) e[t] = Math.random() * 256 | 0;
	} }), t._CastToSize = (e) => BigInt(e), t._NullPointer = 0n, t._PointerSize = 8;
	var i = "./this.program", a = (e, t) => {
		throw t;
	}, o = {}.url, s = "";
	function c(e) {
		return t.locateFile ? t.locateFile(e, s) : s + e;
	}
	var l, u;
	if (n || r) {
		try {
			s = new URL(".", o).href;
		} catch {}
		r && (u = (e) => {
			var t = new XMLHttpRequest();
			return t.open("GET", e, !1), t.responseType = "arraybuffer", t.send(null), new Uint8Array(t.response);
		}), l = async (e) => {
			if (h(e)) return new Promise((t, n) => {
				var r = new XMLHttpRequest();
				r.open("GET", e, !0), r.responseType = "arraybuffer", r.onload = () => {
					if (r.status == 200 || r.status == 0 && r.response) {
						t(r.response);
						return;
					}
					n(r.status);
				}, r.onerror = n, r.send(null);
			});
			var t = await fetch(e, { credentials: "same-origin" });
			if (t.ok) return t.arrayBuffer();
			throw Error(t.status + " : " + t.url);
		};
	}
	var d = console.log.bind(console), f = console.error.bind(console), p, m = !1, h = (e) => e.startsWith("file://");
	class g {}
	class ee extends g {}
	class _ extends g {
		constructor(e) {
			super(), this.excPtr = e;
		}
	}
	function te() {
		return ki.buffer;
	}
	function ne() {
		if (!b?.buffer?.resizable) {
			var e = te();
			b = new Int8Array(e), ot = new Int16Array(e), t.HEAPU8 = P = new Uint8Array(e), Nt = new Uint16Array(e), R = new Int32Array(e), z = new Uint32Array(e), Pn = new Float32Array(e), Fn = new Float64Array(e), B = new BigInt64Array(e), O = new BigUint64Array(e);
		}
	}
	function re() {
		!t.noFSInit && !L.initialized && L.init(), N.init(), io._b(), L.ignorePermissions = !1;
	}
	function v(e) {
		throw e = `Aborted(${e})`, f(e), m = !0, e += ". Build with -sASSERTIONS for more info.", new WebAssembly.RuntimeError(e);
	}
	var ie;
	function ae() {
		return t.locateFile ? c("magick.wasm") : new URL("data:text/plain;base64,").href;
	}
	function oe(e) {
		if (e == ie && p) return new Uint8Array(p);
		if (u) return u(e);
		throw "both async and sync fetching of the wasm failed";
	}
	async function se(e) {
		if (!p) try {
			var t = await l(e);
			return new Uint8Array(t);
		} catch {}
		return oe(e);
	}
	async function ce(e, t) {
		try {
			var n = await se(e);
			return await WebAssembly.instantiate(n, t);
		} catch (e) {
			f(`failed to asynchronously prepare wasm: ${e}`), v(e);
		}
	}
	async function le(e, t, n) {
		if (!e && !h(t)) try {
			var r = fetch(t, { credentials: "same-origin" });
			return await WebAssembly.instantiateStreaming(r, n);
		} catch (e) {
			f(`wasm streaming compile failed: ${e}`), f("falling back to ArrayBuffer instantiation");
		}
		return ce(t, n);
	}
	function ue() {
		return { a: Mi };
	}
	async function y() {
		function e(e) {
			return io = e.exports, io = no(io), ji(io), ne(), io;
		}
		function n(t) {
			return e(t.instance);
		}
		var r = ue(), i = t.instantiateWasm;
		return i ? new Promise((t) => {
			i(r, (n) => t(e(n)));
		}) : (ie ??= ae(), n(await le(p, ie, r)));
	}
	class de {
		name = "ExitStatus";
		constructor(e) {
			this.message = `Program terminated with exit(${e})`, this.status = e;
		}
	}
	var b, x = (e) => Ci(e), S = () => wi(), fe = 9007199254740992, C = -9007199254740992, w = (e) => e < C || e > fe ? NaN : Number(e), pe = [], T = (e) => {
		e = Number(e);
		var t = pe[e];
		return t || (pe[e] = t = Ai.get(BigInt(e))), t;
	};
	function E(e, t) {
		return e = w(e), T(e)(t);
	}
	var me = [], he = 0, ge = function(e) {
		e = w(e);
		var t = (() => {
			var t = new ve(e);
			return t.get_caught() || (t.set_caught(!0), he--), t.set_rethrown(!1), me.push(t), Oi(e);
		})();
		return BigInt(t);
	}, D = null, _e = () => {
		$(0, 0);
		var e = me.pop();
		Ti(e.excPtr), D = null;
	}, O;
	class ve {
		constructor(e) {
			this.excPtr = e, this.ptr = e - 48;
		}
		set_type(e) {
			O[(this.ptr + 8) / 8] = BigInt(e);
		}
		get_type() {
			return Number(O[(this.ptr + 8) / 8]);
		}
		set_destructor(e) {
			O[(this.ptr + 16) / 8] = BigInt(e);
		}
		get_destructor() {
			return Number(O[(this.ptr + 16) / 8]);
		}
		set_caught(e) {
			e = +!!e, b[this.ptr + 24] = e;
		}
		get_caught() {
			return b[this.ptr + 24] != 0;
		}
		set_rethrown(e) {
			e = +!!e, b[this.ptr + 25] = e;
		}
		get_rethrown() {
			return b[this.ptr + 25] != 0;
		}
		init(e, t) {
			this.set_adjusted_ptr(0), this.set_type(e), this.set_destructor(t);
		}
		set_adjusted_ptr(e) {
			O[(this.ptr + 32) / 8] = BigInt(e);
		}
		get_adjusted_ptr() {
			return Number(O[(this.ptr + 32) / 8]);
		}
	}
	var ye = (e) => Si(e), be = (e) => {
		var t = D?.excPtr;
		if (!t) return ye(0), 0;
		var n = new ve(t);
		n.set_adjusted_ptr(t);
		var r = n.get_type();
		if (!r) return ye(0), t;
		for (var i of e) {
			if (!i || i === r) break;
			var a = n.ptr + 32;
			if (Di(i, r, a)) return ye(i), t;
		}
		return ye(r), t;
	}, k = () => BigInt(be([])), xe = (e) => (e = w(e), BigInt(be([e]))), Se = (e, t, n) => (e = w(e), t = w(t), n = w(n), BigInt(be([
		e,
		t,
		n
	]))), Ce = () => {
		me.length || v("no exception to throw");
		var e = me.at(-1), t = e.excPtr;
		throw e.set_rethrown(!0), e.set_caught(!1), he++, Ei(t), D = new _(t), D;
	};
	function we(e, t, n) {
		throw e = w(e), t = w(t), n = w(n), new ve(e).init(t, n), Ei(e), D = new _(e), he++, D;
	}
	var Te = () => he;
	function Ee(e) {
		throw e = w(e), D ||= new _(e), D;
	}
	var A = {
		isAbs: (e) => e.charAt(0) === "/",
		splitPath: (e) => /^(\/?|)([\s\S]*?)((?:\.{1,2}|[^\/]+?|)(\.[^.\/]*|))(?:[\/]*)$/.exec(e).slice(1),
		normalizeArray: (e, t) => {
			for (var n = 0, r = e.length - 1; r >= 0; r--) {
				var i = e[r];
				i === "." ? e.splice(r, 1) : i === ".." ? (e.splice(r, 1), n++) : n && (e.splice(r, 1), n--);
			}
			if (t) for (; n; n--) e.unshift("..");
			return e;
		},
		normalize: (e) => {
			var t = A.isAbs(e), n = e.slice(-1) === "/";
			return e = A.normalizeArray(e.split("/").filter((e) => !!e), !t).join("/"), !e && !t && (e = "."), e && n && (e += "/"), (t ? "/" : "") + e;
		},
		dirname: (e) => {
			var t = A.splitPath(e), n = t[0], r = t[1];
			return !n && !r ? "." : (r &&= r.slice(0, -1), n + r);
		},
		basename: (e) => e && e.match(/([^\/]+|\/)\/*$/)[1],
		join: (...e) => A.normalize(e.join("/")),
		join2: (e, t) => A.normalize(e + "/" + t)
	}, j = () => (e) => (crypto.getRandomValues(e), 0), De = (e) => (De = j())(e), Oe = {
		resolve: (...e) => {
			for (var t = "", n = !1, r = e.length - 1; r >= -1 && !n; r--) {
				var i = r >= 0 ? e[r] : L.cwd();
				if (typeof i != "string") throw TypeError("Arguments to path.resolve must be strings");
				if (!i) return "";
				t = i + "/" + t, n = A.isAbs(i);
			}
			return t = A.normalizeArray(t.split("/").filter((e) => !!e), !n).join("/"), (n ? "/" : "") + t || ".";
		},
		relative: (e, t) => {
			e = Oe.resolve(e).slice(1), t = Oe.resolve(t).slice(1);
			function n(e) {
				for (var t = 0; t < e.length && e[t] === ""; t++);
				for (var n = e.length - 1; n >= 0 && e[n] === ""; n--);
				return t > n ? [] : e.slice(t, n - t + 1);
			}
			for (var r = n(e.split("/")), i = n(t.split("/")), a = Math.min(r.length, i.length), o = a, s = 0; s < a; s++) if (r[s] !== i[s]) {
				o = s;
				break;
			}
			for (var c = [], s = o; s < r.length; s++) c.push("..");
			return c = c.concat(i.slice(o)), c.join("/");
		}
	}, ke = globalThis.TextDecoder && new TextDecoder(), Ae = (e, t, n, r) => {
		var i = t + n;
		if (r) return i;
		for (; e[t] && !(t >= i);) ++t;
		return t;
	}, M = (e, t = 0, n, r) => {
		var i = Ae(e, t, n, r);
		if (i - t > 16 && e.buffer && ke) return ke.decode(e.subarray(t, i));
		for (var a = ""; t < i;) {
			var o = e[t++];
			if (!(o & 128)) {
				a += String.fromCharCode(o);
				continue;
			}
			var s = e[t++] & 63;
			if ((o & 224) == 192) {
				a += String.fromCharCode((o & 31) << 6 | s);
				continue;
			}
			var c = e[t++] & 63;
			if (o = (o & 240) == 224 ? (o & 15) << 12 | s << 6 | c : (o & 7) << 18 | s << 12 | c << 6 | e[t++] & 63, o < 65536) a += String.fromCharCode(o);
			else {
				var l = o - 65536;
				a += String.fromCharCode(55296 | l >> 10, 56320 | l & 1023);
			}
		}
		return a;
	}, je = [], Me = (e) => {
		for (var t = 0, n = 0; n < e.length; ++n) {
			var r = e.charCodeAt(n);
			r <= 127 ? t++ : r <= 2047 ? t += 2 : r >= 55296 && r <= 57343 ? (t += 4, ++n) : t += 3;
		}
		return t;
	}, Ne = (e, t, n, r) => {
		if (!(r > 0)) return 0;
		for (var i = n, a = n + r - 1, o = 0; o < e.length; ++o) {
			var s = e.codePointAt(o);
			if (s <= 127) {
				if (n >= a) break;
				t[n++] = s;
			} else if (s <= 2047) {
				if (n + 1 >= a) break;
				t[n++] = 192 | s >> 6, t[n++] = 128 | s & 63;
			} else if (s <= 65535) {
				if (n + 2 >= a) break;
				t[n++] = 224 | s >> 12, t[n++] = 128 | s >> 6 & 63, t[n++] = 128 | s & 63;
			} else {
				if (n + 3 >= a) break;
				t[n++] = 240 | s >> 18, t[n++] = 128 | s >> 12 & 63, t[n++] = 128 | s >> 6 & 63, t[n++] = 128 | s & 63, o++;
			}
		}
		return t[n] = 0, n - i;
	}, Pe = (e, t, n) => {
		var r = n > 0 ? n : Me(e) + 1, i = Array(r), a = Ne(e, i, 0, i.length);
		return t && (i.length = a), i;
	}, Fe = () => {
		if (!je.length) {
			var e = null;
			if (globalThis.window?.prompt && (e = window.prompt("Input: "), e !== null && (e += "\n")), !e) return null;
			je = Pe(e, !0);
		}
		return je.shift();
	}, N = {
		ttys: [],
		init() {},
		shutdown() {},
		register(e, t) {
			N.ttys[e] = {
				input: [],
				output: [],
				ops: t
			}, L.registerDevice(e, N.stream_ops);
		},
		stream_ops: {
			open(e) {
				var t = N.ttys[e.node.rdev];
				if (!t) throw new L.ErrnoError(43);
				e.tty = t, e.seekable = !1;
			},
			close(e) {
				e.tty.ops.fsync(e.tty);
			},
			fsync(e) {
				e.tty.ops.fsync(e.tty);
			},
			read(e, t, n, r, i) {
				if (!e.tty || !e.tty.ops.get_char) throw new L.ErrnoError(60);
				for (var a = 0, o = 0; o < r; o++) {
					var s;
					try {
						s = e.tty.ops.get_char(e.tty);
					} catch {
						throw new L.ErrnoError(29);
					}
					if (s === void 0 && !a) throw new L.ErrnoError(6);
					if (s == null) break;
					a++, t[n + o] = s;
				}
				return a && (e.node.atime = Date.now()), a;
			},
			write(e, t, n, r, i) {
				if (!e.tty || !e.tty.ops.put_char) throw new L.ErrnoError(60);
				try {
					for (var a = 0; a < r; a++) e.tty.ops.put_char(e.tty, t[n + a]);
				} catch {
					throw new L.ErrnoError(29);
				}
				return r && (e.node.mtime = e.node.ctime = Date.now()), a;
			}
		},
		default_tty_ops: {
			get_char(e) {
				return Fe();
			},
			put_char(e, t) {
				t === null || t === 10 ? (d(M(e.output)), e.output = []) : t != 0 && e.output.push(t);
			},
			fsync(e) {
				e.output?.length > 0 && (d(M(e.output)), e.output = []);
			},
			ioctl_tcgets(e) {
				return {
					c_iflag: 25856,
					c_oflag: 5,
					c_cflag: 191,
					c_lflag: 35387,
					c_cc: [
						3,
						28,
						127,
						21,
						4,
						0,
						1,
						0,
						17,
						19,
						26,
						0,
						18,
						15,
						23,
						22,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0
					]
				};
			},
			ioctl_tcsets(e, t, n) {
				return 0;
			},
			ioctl_tiocgwinsz(e) {
				return [24, 80];
			}
		},
		default_tty1_ops: {
			put_char(e, t) {
				t === null || t === 10 ? (f(M(e.output)), e.output = []) : t != 0 && e.output.push(t);
			},
			fsync(e) {
				e.output?.length > 0 && (f(M(e.output)), e.output = []);
			}
		}
	}, P, Ie = (e, t) => P.fill(0, e, e + t), Le = (e, t) => Math.ceil(e / t) * t, Re = (e) => {
		e = Le(e, 65536);
		var t = xi(65536, e);
		return t && Ie(t, e), t;
	}, F = {
		ops_table: null,
		mount(e) {
			return F.createNode(null, "/", 16895, 0);
		},
		createNode(e, t, n, r) {
			if (L.isBlkdev(n) || L.isFIFO(n)) throw new L.ErrnoError(63);
			F.ops_table ||= {
				dir: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr,
						lookup: F.node_ops.lookup,
						mknod: F.node_ops.mknod,
						rename: F.node_ops.rename,
						unlink: F.node_ops.unlink,
						rmdir: F.node_ops.rmdir,
						readdir: F.node_ops.readdir,
						symlink: F.node_ops.symlink
					},
					stream: { llseek: F.stream_ops.llseek }
				},
				file: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr
					},
					stream: {
						llseek: F.stream_ops.llseek,
						read: F.stream_ops.read,
						write: F.stream_ops.write,
						mmap: F.stream_ops.mmap,
						msync: F.stream_ops.msync
					}
				},
				link: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr,
						readlink: F.node_ops.readlink
					},
					stream: {}
				},
				chrdev: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr
					},
					stream: L.chrdev_stream_ops
				}
			};
			var i = L.createNode(e, t, n, r);
			return L.isDir(i.mode) ? (i.node_ops = F.ops_table.dir.node, i.stream_ops = F.ops_table.dir.stream, i.contents = {}) : L.isFile(i.mode) ? (i.node_ops = F.ops_table.file.node, i.stream_ops = F.ops_table.file.stream, i.usedBytes = 0, i.contents = F.emptyFileContents ??= /* @__PURE__ */ new Uint8Array()) : L.isLink(i.mode) ? (i.node_ops = F.ops_table.link.node, i.stream_ops = F.ops_table.link.stream) : L.isChrdev(i.mode) && (i.node_ops = F.ops_table.chrdev.node, i.stream_ops = F.ops_table.chrdev.stream), i.atime = i.mtime = i.ctime = Date.now(), e && (e.contents[t] = i, e.atime = e.mtime = e.ctime = i.atime), i;
		},
		getFileDataAsTypedArray(e) {
			return e.contents.subarray(0, e.usedBytes);
		},
		expandFileStorage(e, t) {
			var n = e.contents.length;
			if (!(n >= t)) {
				t = Math.max(t, n * (n < 1048576 ? 2 : 1.125) >>> 0), n && (t = Math.max(t, 256));
				var r = F.getFileDataAsTypedArray(e);
				e.contents = new Uint8Array(t), e.contents.set(r);
			}
		},
		resizeFileStorage(e, t) {
			if (e.usedBytes != t) {
				var n = e.contents;
				e.contents = new Uint8Array(t), e.contents.set(n.subarray(0, Math.min(t, e.usedBytes))), e.usedBytes = t;
			}
		},
		node_ops: {
			getattr(e) {
				var t = {};
				return t.dev = L.isChrdev(e.mode) ? e.id : 1, t.ino = e.id, t.mode = e.mode, t.nlink = 1, t.uid = 0, t.gid = 0, t.rdev = e.rdev, t.size = L.isDir(e.mode) ? 4096 : L.isFile(e.mode) ? e.usedBytes : L.isLink(e.mode) ? e.link.length : 0, t.atime = new Date(e.atime), t.mtime = new Date(e.mtime), t.ctime = new Date(e.ctime), t.blksize = 4096, t.blocks = Math.ceil(t.size / t.blksize), t;
			},
			setattr(e, t) {
				for (let n of [
					"mode",
					"atime",
					"mtime",
					"ctime"
				]) t[n] != null && (e[n] = t[n]);
				t.size !== void 0 && F.resizeFileStorage(e, t.size);
			},
			lookup(e, t) {
				throw F.doesNotExistError || (F.doesNotExistError = new L.ErrnoError(44), F.doesNotExistError.stack = "<generic error, no stack>"), F.doesNotExistError;
			},
			mknod(e, t, n, r) {
				return F.createNode(e, t, n, r);
			},
			rename(e, t, n) {
				var r;
				try {
					r = L.lookupNode(t, n);
				} catch {}
				if (r) {
					if (L.isDir(e.mode)) for (var i in r.contents) throw new L.ErrnoError(55);
					L.hashRemoveNode(r);
				}
				delete e.parent.contents[e.name], t.contents[n] = e, e.name = n, t.ctime = t.mtime = e.parent.ctime = e.parent.mtime = Date.now();
			},
			unlink(e, t) {
				delete e.contents[t], e.ctime = e.mtime = Date.now();
			},
			rmdir(e, t) {
				for (var n in L.lookupNode(e, t).contents) throw new L.ErrnoError(55);
				delete e.contents[t], e.ctime = e.mtime = Date.now();
			},
			readdir(e) {
				return [
					".",
					"..",
					...Object.keys(e.contents)
				];
			},
			symlink(e, t, n) {
				var r = F.createNode(e, t, 41471, 0);
				return r.link = n, r;
			},
			readlink(e) {
				if (!L.isLink(e.mode)) throw new L.ErrnoError(28);
				return e.link;
			}
		},
		stream_ops: {
			read(e, t, n, r, i) {
				var a = e.node.contents;
				if (i >= e.node.usedBytes) return 0;
				var o = Math.min(e.node.usedBytes - i, r);
				return t.set(a.subarray(i, i + o), n), o;
			},
			write(e, t, n, r, i, a) {
				if (t.buffer === b.buffer && (a = !1), !r) return 0;
				var o = e.node;
				return o.mtime = o.ctime = Date.now(), a ? (o.contents = t.subarray(n, n + r), o.usedBytes = r) : !o.usedBytes && !i ? (o.contents = t.slice(n, n + r), o.usedBytes = r) : (F.expandFileStorage(o, i + r), o.contents.set(t.subarray(n, n + r), i), o.usedBytes = Math.max(o.usedBytes, i + r)), r;
			},
			llseek(e, t, n) {
				var r = t;
				if (n === 1 ? r += e.position : n === 2 && L.isFile(e.node.mode) && (r += e.node.usedBytes), r < 0) throw new L.ErrnoError(28);
				return r;
			},
			mmap(e, t, n, r, i) {
				if (!L.isFile(e.node.mode)) throw new L.ErrnoError(43);
				var a, o, s = e.node.contents;
				if (!(i & 2) && s.buffer === b.buffer) o = !1, a = s.byteOffset;
				else {
					if (o = !0, a = Re(t), !a) throw new L.ErrnoError(48);
					s && ((n > 0 || n + t < s.length) && (s = s.subarray ? s.subarray(n, n + t) : Array.prototype.slice.call(s, n, n + t)), b.set(s, a));
				}
				return {
					ptr: a,
					allocated: o
				};
			},
			msync(e, t, n, r, i) {
				return F.stream_ops.write(e, t, 0, r, n, !1), 0;
			}
		}
	}, I = (e) => {
		if (typeof e != "string") return e;
		var t = {
			r: 0,
			"r+": 2,
			w: 577,
			"w+": 578,
			a: 1089,
			"a+": 1090
		}[e];
		if (t === void 0) throw Error(`Unknown file open mode: ${e}`);
		return t;
	}, ze = (e) => (typeof e == "string" && (e = Pe(e, !0)), e.subarray || (e = new Uint8Array(e)), e), Be = (e, t) => {
		var n = 0;
		return e && (n |= 365), t && (n |= 146), n;
	}, Ve = async (e) => {
		var t = await l(e);
		return new Uint8Array(t);
	}, He = (...e) => L.createDataFile(...e), Ue = (e) => e, We = null, Ge = async () => We, Ke = 0, qe = null, Je = (e) => {
		Ke--, Ke || qe();
	}, Ye = (e) => {
		Ke || (We = new Promise((e) => qe = e)), Ke++;
	}, Xe = [], Ze = async (e, t) => {
		typeof Browser < "u" && Browser.init();
		for (var n of Xe) if (n.canHandle(t)) return n.handle(e, t);
		return e;
	}, Qe = async (e, t, n, r, i, a, o, s) => {
		var c = t ? Oe.resolve(A.join2(e, t)) : e, l = Ue(`cp ${c}`);
		Ye(l);
		try {
			var u = n;
			typeof n == "string" && (u = await Ve(n)), u = await Ze(u, c), s?.(), a || He(e, t, u, r, i, o);
		} finally {
			Je(l);
		}
	}, $e = (e, t, n, r, i, a, o, s, c, l) => {
		Qe(e, t, n, r, i, s, c, l).then(a).catch(o);
	}, L = {
		root: null,
		mounts: [],
		devices: {},
		streams: [],
		nextInode: 1,
		nameTable: null,
		currentPath: "/",
		initialized: !1,
		ignorePermissions: !0,
		filesystems: null,
		syncFSRequests: 0,
		ErrnoError: class {
			name = "ErrnoError";
			constructor(e) {
				this.errno = e;
			}
		},
		FSStream: class {
			shared = {};
			get object() {
				return this.node;
			}
			set object(e) {
				this.node = e;
			}
			get isRead() {
				return (this.flags & 2097155) != 1;
			}
			get isWrite() {
				return !!(this.flags & 2097155);
			}
			get isAppend() {
				return this.flags & 1024;
			}
			get flags() {
				return this.shared.flags;
			}
			set flags(e) {
				this.shared.flags = e;
			}
			get position() {
				return this.shared.position;
			}
			set position(e) {
				this.shared.position = e;
			}
		},
		FSNode: class {
			node_ops = {};
			stream_ops = {};
			readMode = 365;
			writeMode = 146;
			mounted = null;
			constructor(e, t, n, r) {
				e ||= this, this.parent = e, this.mount = e.mount, this.id = L.nextInode++, this.name = t, this.mode = n, this.rdev = r, this.atime = this.mtime = this.ctime = Date.now();
			}
			get read() {
				return (this.mode & this.readMode) === this.readMode;
			}
			set read(e) {
				e ? this.mode |= this.readMode : this.mode &= ~this.readMode;
			}
			get write() {
				return (this.mode & this.writeMode) === this.writeMode;
			}
			set write(e) {
				e ? this.mode |= this.writeMode : this.mode &= ~this.writeMode;
			}
			get isFolder() {
				return L.isDir(this.mode);
			}
			get isDevice() {
				return L.isChrdev(this.mode);
			}
			addListener(e, t = !1) {
				var n = {
					cb: e,
					exclusive: t
				}, r = this.listeners ??= /* @__PURE__ */ new Set();
				return r.add(n), {
					listeners: r,
					entry: n
				};
			}
			notifyListeners(e) {
				if (this.listeners) {
					var t;
					for (var n of this.listeners) n.exclusive ? (t ||= []).push(n) : n.cb(e);
					if (t) {
						var r = (this.exclTurn || 0) % t.length;
						this.exclTurn = r + 1, t[r].cb(e);
					}
				}
			}
		},
		lookupPath(e, t = {}) {
			if (!e) throw new L.ErrnoError(44);
			t.follow_mount ??= !0, A.isAbs(e) || (e = L.cwd() + "/" + e);
			linkloop: for (var n = 0; n < 40; n++) {
				for (var r = e.split("/").filter((e) => !!e), i = L.root, a = "/", o = 0; o < r.length; o++) {
					var s = o === r.length - 1;
					if (s && t.parent) break;
					if (r[o] !== ".") {
						if (r[o] === "..") {
							if (a = A.dirname(a), L.isRoot(i)) {
								e = a + "/" + r.slice(o + 1).join("/"), n--;
								continue linkloop;
							}
							i = i.parent;
							continue;
						}
						a = A.join2(a, r[o]);
						try {
							i = L.lookupNode(i, r[o]);
						} catch (e) {
							if (e?.errno === 44 && s && t.noent_okay) return { path: a };
							throw e;
						}
						if (L.isMountpoint(i) && (!s || t.follow_mount) && (i = i.mounted.root), L.isLink(i.mode) && (!s || t.follow)) {
							if (!i.node_ops.readlink) throw new L.ErrnoError(52);
							var c = i.node_ops.readlink(i);
							A.isAbs(c) || (c = A.dirname(a) + "/" + c), e = c + "/" + r.slice(o + 1).join("/");
							continue linkloop;
						}
					}
				}
				return {
					path: a,
					node: i
				};
			}
			throw new L.ErrnoError(32);
		},
		getPath(e) {
			for (var t;;) {
				if (L.isRoot(e)) {
					var n = e.mount.mountpoint;
					return t ? n[n.length - 1] === "/" ? n + t : `${n}/${t}` : n;
				}
				t = t ? `${e.name}/${t}` : e.name, e = e.parent;
			}
		},
		hashName(e, t) {
			for (var n = 0, r = 0; r < t.length; r++) n = (n << 5) - n + t.charCodeAt(r) | 0;
			return (e + n >>> 0) % L.nameTable.length;
		},
		hashAddNode(e) {
			var t = L.hashName(e.parent.id, e.name);
			e.name_next = L.nameTable[t], L.nameTable[t] = e;
		},
		hashRemoveNode(e) {
			var t = L.hashName(e.parent.id, e.name);
			if (L.nameTable[t] === e) L.nameTable[t] = e.name_next;
			else for (var n = L.nameTable[t]; n;) {
				if (n.name_next === e) {
					n.name_next = e.name_next;
					break;
				}
				n = n.name_next;
			}
		},
		lookupNode(e, t) {
			var n = L.mayLookup(e);
			if (n) throw new L.ErrnoError(n);
			for (var r = L.hashName(e.id, t), i = L.nameTable[r]; i; i = i.name_next) {
				var a = i.name;
				if (i.parent.id === e.id && a === t) return i;
			}
			return L.lookup(e, t);
		},
		createNode(e, t, n, r) {
			var i = new L.FSNode(e, t, n, r);
			return L.hashAddNode(i), i;
		},
		destroyNode(e) {
			L.hashRemoveNode(e);
		},
		isRoot(e) {
			return e === e.parent;
		},
		isMountpoint(e) {
			return !!e.mounted;
		},
		isFile(e) {
			return (e & 61440) == 32768;
		},
		isDir(e) {
			return (e & 61440) == 16384;
		},
		isLink(e) {
			return (e & 61440) == 40960;
		},
		isChrdev(e) {
			return (e & 61440) == 8192;
		},
		isBlkdev(e) {
			return (e & 61440) == 24576;
		},
		isFIFO(e) {
			return (e & 61440) == 4096;
		},
		isSocket(e) {
			return (e & 49152) == 49152;
		},
		flagsToPermissionString(e) {
			var t = [
				"r",
				"w",
				"rw"
			][e & 3];
			return e & 512 && (t += "w"), t;
		},
		nodePermissions(e, t) {
			return L.ignorePermissions ? 0 : t.includes("r") && !(e.mode & 292) || t.includes("w") && !(e.mode & 146) || t.includes("x") && !(e.mode & 73) ? 2 : 0;
		},
		mayLookup(e) {
			return L.isDir(e.mode) ? L.nodePermissions(e, "x") || (e.node_ops.lookup ? 0 : 2) : 54;
		},
		mayCreate(e, t) {
			if (!L.isDir(e.mode)) return 54;
			try {
				return L.lookupNode(e, t), 20;
			} catch {}
			return L.nodePermissions(e, "wx");
		},
		mayDelete(e, t, n) {
			var r;
			try {
				r = L.lookupNode(e, t);
			} catch (e) {
				return e.errno;
			}
			var i = L.nodePermissions(e, "wx");
			if (i) return i;
			if (n) {
				if (!L.isDir(r.mode)) return 54;
				if (L.isRoot(r) || L.getPath(r) === L.cwd()) return 10;
			} else if (L.isDir(r.mode)) return 31;
			return 0;
		},
		mayOpen(e, t) {
			if (!e) return 44;
			if (L.isLink(e.mode)) return 32;
			var n = L.flagsToPermissionString(t);
			return L.isDir(e.mode) && (n !== "r" || t & 576) ? 31 : L.nodePermissions(e, n);
		},
		checkOpExists(e, t) {
			if (!e) throw new L.ErrnoError(t);
			return e;
		},
		MAX_OPEN_FDS: 4096,
		nextfd() {
			for (var e = 0; e <= L.MAX_OPEN_FDS; e++) if (!L.streams[e]) return e;
			throw new L.ErrnoError(33);
		},
		getStreamChecked(e) {
			var t = L.getStream(e);
			if (!t) throw new L.ErrnoError(8);
			return t;
		},
		getStream: (e) => L.streams[e],
		createStream(e, t = -1) {
			return e = Object.assign(new L.FSStream(), e), t == -1 && (t = L.nextfd()), e.fd = t, L.streams[t] = e, e;
		},
		closeStream(e) {
			L.streams[e] = null;
		},
		dupStream(e, t = -1) {
			var n = L.createStream(e, t);
			return n.stream_ops?.dup?.(n), n;
		},
		doSetAttr(e, t, n) {
			var r = e?.stream_ops.setattr, i = r ? e : t;
			r ??= t.node_ops.setattr, L.checkOpExists(r, 63);
			try {
				r(i, n);
			} catch (e) {
				throw e instanceof RangeError ? new L.ErrnoError(22) : e;
			}
		},
		chrdev_stream_ops: {
			open(e) {
				e.stream_ops = L.getDevice(e.node.rdev).stream_ops, e.stream_ops.open?.(e);
			},
			llseek() {
				throw new L.ErrnoError(70);
			}
		},
		major: (e) => e >> 8,
		minor: (e) => e & 255,
		makedev: (e, t) => e << 8 | t,
		registerDevice(e, t) {
			L.devices[e] = { stream_ops: t };
		},
		getDevice: (e) => L.devices[e],
		getMounts(e) {
			for (var t = [], n = [e]; n.length;) {
				var r = n.pop();
				t.push(r), n.push(...r.mounts);
			}
			return t;
		},
		syncfs(e, t) {
			typeof e == "function" && (t = e, e = !1), L.syncFSRequests++, L.syncFSRequests > 1 && f(`warning: ${L.syncFSRequests} FS.syncfs operations in flight at once, probably just doing extra work`);
			var n = L.getMounts(L.root.mount), r = 0;
			function i(e) {
				return L.syncFSRequests--, t(e);
			}
			function a(e) {
				if (e) return a.errored ? void 0 : (a.errored = !0, i(e));
				++r >= n.length && i(null);
			}
			for (var o of n) o.type.syncfs ? o.type.syncfs(o, e, a) : a(null);
		},
		mount(e, t, n) {
			var r = n === "/", i = !n, a;
			if (r && L.root) throw new L.ErrnoError(10);
			if (!r && !i) {
				var o = L.lookupPath(n, { follow_mount: !1 });
				if (n = o.path, a = o.node, L.isMountpoint(a)) throw new L.ErrnoError(10);
				if (!L.isDir(a.mode)) throw new L.ErrnoError(54);
			}
			var s = {
				type: e,
				opts: t,
				mountpoint: n,
				mounts: []
			}, c = e.mount(s);
			return c.mount = s, s.root = c, r ? L.root = c : a && (a.mounted = s, a.mount && a.mount.mounts.push(s)), c;
		},
		unmount(e) {
			var t = L.lookupPath(e, { follow_mount: !1 });
			if (!L.isMountpoint(t.node)) throw new L.ErrnoError(28);
			var n = t.node, r = n.mounted, i = L.getMounts(r);
			for (var [a, o] of Object.entries(L.nameTable)) for (; o;) {
				var s = o.name_next;
				i.includes(o.mount) && L.destroyNode(o), o = s;
			}
			n.mounted = null;
			var c = n.mount.mounts.indexOf(r);
			n.mount.mounts.splice(c, 1);
		},
		lookup(e, t) {
			return e.node_ops.lookup(e, t);
		},
		mknod(e, t, n) {
			var r = L.lookupPath(e, { parent: !0 }).node, i = A.basename(e);
			if (!i) throw new L.ErrnoError(28);
			if (i === "." || i === "..") throw new L.ErrnoError(20);
			var a = L.mayCreate(r, i);
			if (a) throw new L.ErrnoError(a);
			if (!r.node_ops.mknod) throw new L.ErrnoError(63);
			return r.node_ops.mknod(r, i, t, n);
		},
		statfs(e) {
			return L.statfsNode(L.lookupPath(e, { follow: !0 }).node);
		},
		statfsStream(e) {
			return L.statfsNode(e.node);
		},
		statfsNode(e) {
			var t = {
				bsize: 4096,
				frsize: 4096,
				blocks: 1e6,
				bfree: 5e5,
				bavail: 5e5,
				files: L.nextInode,
				ffree: L.nextInode - 1,
				fsid: 42,
				flags: 2,
				namelen: 255
			};
			return e.node_ops.statfs && Object.assign(t, e.node_ops.statfs(e.mount.opts.root)), t;
		},
		create(e, t = 438) {
			return t &= 4095, t |= 32768, L.mknod(e, t, 0);
		},
		mkdir(e, t = 511) {
			return t &= 1023, t |= 16384, L.mknod(e, t, 0);
		},
		mkdirTree(e, t) {
			var n = e.split("/"), r = "";
			for (var i of n) if (i) {
				(r || A.isAbs(e)) && (r += "/"), r += i;
				try {
					L.mkdir(r, t);
				} catch (e) {
					if (e.errno != 20) throw e;
				}
			}
		},
		mkdev(e, t, n) {
			return n === void 0 && (n = t, t = 438), t |= 8192, L.mknod(e, t, n);
		},
		symlink(e, t) {
			if (!Oe.resolve(e)) throw new L.ErrnoError(44);
			var n = L.lookupPath(t, { parent: !0 }).node;
			if (!n) throw new L.ErrnoError(44);
			var r = A.basename(t), i = L.mayCreate(n, r);
			if (i) throw new L.ErrnoError(i);
			if (!n.node_ops.symlink) throw new L.ErrnoError(63);
			return n.node_ops.symlink(n, r, e);
		},
		link(e, t, n) {
			var r = L.lookupPath(t, { parent: !0 }).node;
			if (!r) throw new L.ErrnoError(44);
			var i = A.basename(t), a = L.mayCreate(r, i);
			if (a) throw new L.ErrnoError(a);
			if (!r.node_ops.link) throw new L.ErrnoError(34);
			return r.node_ops.link(r, i, e, n);
		},
		rename(e, t) {
			var n = A.dirname(e), r = A.dirname(t), i = A.basename(e), a = A.basename(t), o = L.lookupPath(e, { parent: !0 }), s = o.node, c;
			if (o = L.lookupPath(t, { parent: !0 }), c = o.node, !s || !c) throw new L.ErrnoError(44);
			if (s.mount !== c.mount) throw new L.ErrnoError(75);
			var l = L.lookupNode(s, i), u = Oe.relative(e, r);
			if (u.charAt(0) !== ".") throw new L.ErrnoError(28);
			if (u = Oe.relative(t, n), u.charAt(0) !== ".") throw new L.ErrnoError(55);
			var d;
			try {
				d = L.lookupNode(c, a);
			} catch {}
			if (l !== d) {
				var f = L.isDir(l.mode), p = L.mayDelete(s, i, f);
				if (p || (p = d ? L.mayDelete(c, a, f) : L.mayCreate(c, a), p)) throw new L.ErrnoError(p);
				if (!s.node_ops.rename) throw new L.ErrnoError(63);
				if (L.isMountpoint(l) || d && L.isMountpoint(d)) throw new L.ErrnoError(10);
				if (c !== s && (p = L.nodePermissions(s, "w"), p)) throw new L.ErrnoError(p);
				L.hashRemoveNode(l);
				try {
					s.node_ops.rename(l, c, a), l.parent = c;
				} catch (e) {
					throw e;
				} finally {
					L.hashAddNode(l);
				}
			}
		},
		rmdir(e) {
			var t = L.lookupPath(e, { parent: !0 }).node, n = A.basename(e), r = L.lookupNode(t, n), i = L.mayDelete(t, n, !0);
			if (i) throw new L.ErrnoError(i);
			if (!t.node_ops.rmdir) throw new L.ErrnoError(63);
			if (L.isMountpoint(r)) throw new L.ErrnoError(10);
			t.node_ops.rmdir(t, n), L.destroyNode(r);
		},
		readdir(e) {
			var t = L.lookupPath(e, { follow: !0 }).node;
			return L.checkOpExists(t.node_ops.readdir, 54)(t);
		},
		unlink(e) {
			var t = L.lookupPath(e, { parent: !0 }).node;
			if (!t) throw new L.ErrnoError(44);
			var n = A.basename(e), r = L.lookupNode(t, n), i = L.mayDelete(t, n, !1);
			if (i) throw new L.ErrnoError(i);
			if (!t.node_ops.unlink) throw new L.ErrnoError(63);
			if (L.isMountpoint(r)) throw new L.ErrnoError(10);
			t.node_ops.unlink(t, n), L.destroyNode(r);
		},
		readlink(e) {
			var t = L.lookupPath(e).node;
			if (!t) throw new L.ErrnoError(44);
			if (!t.node_ops.readlink) throw new L.ErrnoError(28);
			return t.node_ops.readlink(t);
		},
		stat(e, t) {
			var n = L.lookupPath(e, { follow: !t }).node;
			return L.checkOpExists(n.node_ops.getattr, 63)(n);
		},
		fstat(e) {
			var t = L.getStreamChecked(e), n = t.node, r = t.stream_ops.getattr, i = r ? t : n;
			return r ??= n.node_ops.getattr, L.checkOpExists(r, 63), r(i);
		},
		lstat(e) {
			return L.stat(e, !0);
		},
		doChmod(e, t, n, r) {
			L.doSetAttr(e, t, {
				mode: n & 4095 | t.mode & -4096,
				ctime: Date.now(),
				dontFollow: r
			});
		},
		chmod(e, t, n) {
			var r = typeof e == "string" ? L.lookupPath(e, { follow: !n }).node : e;
			L.doChmod(null, r, t, n);
		},
		lchmod(e, t) {
			L.chmod(e, t, !0);
		},
		fchmod(e, t) {
			var n = L.getStreamChecked(e);
			L.doChmod(n, n.node, t, !1);
		},
		doChown(e, t, n) {
			L.doSetAttr(e, t, {
				timestamp: Date.now(),
				dontFollow: n
			});
		},
		chown(e, t, n, r) {
			var i = typeof e == "string" ? L.lookupPath(e, { follow: !r }).node : e;
			L.doChown(null, i, r);
		},
		lchown(e, t, n) {
			L.chown(e, t, n, !0);
		},
		fchown(e, t, n) {
			var r = L.getStreamChecked(e);
			L.doChown(r, r.node, !1);
		},
		doTruncate(e, t, n) {
			if (L.isDir(t.mode)) throw new L.ErrnoError(31);
			if (!L.isFile(t.mode)) throw new L.ErrnoError(28);
			var r = L.nodePermissions(t, "w");
			if (r) throw new L.ErrnoError(r);
			L.doSetAttr(e, t, {
				size: n,
				timestamp: Date.now()
			});
		},
		truncate(e, t) {
			if (t < 0) throw new L.ErrnoError(28);
			var n = typeof e == "string" ? L.lookupPath(e, { follow: !0 }).node : e;
			L.doTruncate(null, n, t);
		},
		ftruncate(e, t) {
			var n = L.getStreamChecked(e);
			if (t < 0 || !(n.flags & 2097155)) throw new L.ErrnoError(28);
			L.doTruncate(n, n.node, t);
		},
		utime(e, t, n, r) {
			var i = L.lookupPath(e, { follow: !r });
			L.doSetAttr(null, i.node, {
				atime: t,
				mtime: n,
				dontFollow: r
			});
		},
		open(e, t, n = 438) {
			if (e === "") throw new L.ErrnoError(44);
			t = I(t), n = t & 64 ? n & 4095 | 32768 : 0;
			var r, i;
			if (typeof e == "object") r = e;
			else {
				i = e.endsWith("/");
				var a = L.lookupPath(e, {
					follow: !(t & 131072),
					noent_okay: !0
				});
				r = a.node, e = a.path;
			}
			var o = !1;
			if (t & 64) {
				if (r) {
					if (t & 128) throw new L.ErrnoError(20);
				} else if (i) throw new L.ErrnoError(31);
				else r = L.mknod(e, n | 511, 0), o = !0;
			}
			if (!r) throw new L.ErrnoError(44);
			if (L.isChrdev(r.mode) && (t &= -513), t & 65536 && !L.isDir(r.mode)) throw new L.ErrnoError(54);
			if (!o) {
				var s = L.mayOpen(r, t);
				if (s) throw new L.ErrnoError(s);
			}
			t & 512 && !o && L.truncate(r, 0), t &= -131713;
			var c = L.createStream({
				node: r,
				path: L.getPath(r),
				flags: t,
				seekable: !0,
				position: 0,
				stream_ops: r.stream_ops,
				ungotten: [],
				error: !1
			});
			return c.stream_ops.open && c.stream_ops.open(c), o && L.chmod(r, n & 511), c;
		},
		close(e) {
			if (L.isClosed(e)) throw new L.ErrnoError(8);
			e.getdents &&= null, e.node?.notifyListeners(32);
			try {
				e.stream_ops.close && e.stream_ops.close(e);
			} catch (e) {
				throw e;
			} finally {
				L.closeStream(e.fd);
			}
			e.fd = null;
		},
		isClosed(e) {
			return e.fd === null;
		},
		llseek(e, t, n) {
			if (L.isClosed(e)) throw new L.ErrnoError(8);
			if (!e.seekable || !e.stream_ops.llseek) throw new L.ErrnoError(70);
			if (n != 0 && n != 1 && n != 2) throw new L.ErrnoError(28);
			return e.position = e.stream_ops.llseek(e, t, n), e.ungotten = [], e.position;
		},
		read(e, t, n, r, i) {
			if (r < 0 || i < 0) throw new L.ErrnoError(28);
			if (L.isClosed(e) || (e.flags & 2097155) == 1) throw new L.ErrnoError(8);
			if (L.isDir(e.node.mode)) throw new L.ErrnoError(31);
			if (!e.stream_ops.read) throw new L.ErrnoError(28);
			var a = i !== void 0;
			if (!a) i = e.position;
			else if (!e.seekable) throw new L.ErrnoError(70);
			var o = e.stream_ops.read(e, t, n, r, i);
			return a || (e.position += o), o;
		},
		write(e, t, n, r, i, a) {
			if (r < 0 || i < 0) throw new L.ErrnoError(28);
			if (L.isClosed(e) || !(e.flags & 2097155)) throw new L.ErrnoError(8);
			if (L.isDir(e.node.mode)) throw new L.ErrnoError(31);
			if (!e.stream_ops.write) throw new L.ErrnoError(28);
			e.seekable && e.flags & 1024 && L.llseek(e, 0, 2);
			var o = i !== void 0;
			if (!o) i = e.position;
			else if (!e.seekable) throw new L.ErrnoError(70);
			var s = e.stream_ops.write(e, t, n, r, i, a);
			return o || (e.position += s), s;
		},
		mmap(e, t, n, r, i) {
			if (r & 2 && !(i & 2) && (e.flags & 2097155) != 2 || (e.flags & 2097155) == 1) throw new L.ErrnoError(2);
			if (!e.stream_ops.mmap) throw new L.ErrnoError(43);
			if (!t) throw new L.ErrnoError(28);
			return e.stream_ops.mmap(e, t, n, r, i);
		},
		msync(e, t, n, r, i) {
			return e.stream_ops.msync ? e.stream_ops.msync(e, t, n, r, i) : 0;
		},
		ioctl(e, t, n) {
			if (!e.stream_ops.ioctl) throw new L.ErrnoError(59);
			return e.stream_ops.ioctl(e, t, n);
		},
		readFile(e, t = {}) {
			t.flags = t.flags ?? 0, t.encoding = t.encoding ?? "binary", t.encoding !== "utf8" && t.encoding !== "binary" && v(`Invalid encoding type "${t.encoding}"`);
			var n = L.open(e, t.flags), r = L.stat(e).size, i = new Uint8Array(r);
			return L.read(n, i, 0, r, 0), t.encoding === "utf8" && (i = M(i)), L.close(n), i;
		},
		writeFile(e, t, n = {}) {
			n.flags = n.flags ?? 577;
			var r = L.open(e, n.flags, n.mode);
			t = ze(t), L.write(r, t, 0, t.byteLength, void 0, n.canOwn), L.close(r);
		},
		cwd: () => L.currentPath,
		chdir(e) {
			var t = L.lookupPath(e, { follow: !0 });
			if (t.node === null) throw new L.ErrnoError(44);
			if (!L.isDir(t.node.mode)) throw new L.ErrnoError(54);
			var n = L.nodePermissions(t.node, "x");
			if (n) throw new L.ErrnoError(n);
			L.currentPath = t.path;
		},
		createDefaultDirectories() {
			L.mkdir("/tmp"), L.mkdir("/home"), L.mkdir("/home/web_user");
		},
		createDefaultDevices() {
			L.mkdir("/dev"), L.registerDevice(L.makedev(1, 3), {
				read: () => 0,
				write: (e, t, n, r, i) => r,
				llseek: () => 0
			}), L.mkdev("/dev/null", L.makedev(1, 3)), N.register(L.makedev(5, 0), N.default_tty_ops), N.register(L.makedev(6, 0), N.default_tty1_ops), L.mkdev("/dev/tty", L.makedev(5, 0)), L.mkdev("/dev/tty1", L.makedev(6, 0));
			var e = /* @__PURE__ */ new Uint8Array(1024), t = 0, n = () => (t ||= (De(e), e.byteLength), e[--t]);
			L.createDevice("/dev", "random", n), L.createDevice("/dev", "urandom", n), L.mkdir("/dev/shm"), L.mkdir("/dev/shm/tmp");
		},
		createSpecialDirectories() {
			L.mkdir("/proc");
			var e = L.mkdir("/proc/self");
			L.mkdir("/proc/self/fd"), L.mount({ mount() {
				var t = L.createNode(e, "fd", 16895, 73);
				return t.stream_ops = { llseek: F.stream_ops.llseek }, t.node_ops = {
					lookup(e, t) {
						var n = +t, r = L.getStreamChecked(n), i = {
							parent: null,
							mount: { mountpoint: "fake" },
							node_ops: { readlink: () => r.path },
							id: n + 1
						};
						return i.parent = i, i;
					},
					readdir() {
						return Array.from(L.streams.entries()).filter(([e, t]) => t).map(([e, t]) => e.toString());
					}
				}, t;
			} }, {}, "/proc/self/fd");
		},
		createStandardStreams(e, t, n) {
			e ? L.createDevice("/dev", "stdin", e) : L.symlink("/dev/tty", "/dev/stdin"), t ? L.createDevice("/dev", "stdout", null, t) : L.symlink("/dev/tty", "/dev/stdout"), n ? L.createDevice("/dev", "stderr", null, n) : L.symlink("/dev/tty1", "/dev/stderr"), L.open("/dev/stdin", 0), L.open("/dev/stdout", 1), L.open("/dev/stderr", 1);
		},
		staticInit() {
			L.nameTable = Array(4096), L.mount(F, {}, "/"), L.createDefaultDirectories(), L.createDefaultDevices(), L.createSpecialDirectories(), L.filesystems = { MEMFS: F };
		},
		init(e, t, n) {
			L.initialized = !0, L.createStandardStreams(e, t, n);
		},
		quit() {
			L.initialized = !1;
			for (var e of L.streams) e && L.close(e);
		},
		findObject(e, t) {
			var n = L.analyzePath(e, t);
			return n.exists ? n.object : null;
		},
		analyzePath(e, t) {
			try {
				var n = L.lookupPath(e, { follow: !t });
				e = n.path;
			} catch {}
			var r = {
				isRoot: !1,
				exists: !1,
				error: 0,
				name: null,
				path: null,
				object: null,
				parentExists: !1,
				parentPath: null,
				parentObject: null
			};
			try {
				var n = L.lookupPath(e, { parent: !0 });
				r.parentExists = !0, r.parentPath = n.path, r.parentObject = n.node, r.name = A.basename(e), n = L.lookupPath(e, { follow: !t }), r.exists = !0, r.path = n.path, r.object = n.node, r.name = n.node.name, r.isRoot = n.path === "/";
			} catch (e) {
				r.error = e.errno;
			}
			return r;
		},
		createPath(e, t, n, r) {
			e = typeof e == "string" ? e : L.getPath(e);
			for (var i = t.split("/").reverse(); i.length;) {
				var a = i.pop();
				if (a) {
					var o = A.join2(e, a);
					try {
						L.mkdir(o);
					} catch (e) {
						if (e.errno != 20) throw e;
					}
					e = o;
				}
			}
			return o;
		},
		createFile(e, t, n, r, i) {
			var a = A.join2(typeof e == "string" ? e : L.getPath(e), t), o = Be(r, i);
			return L.create(a, o);
		},
		createDataFile(e, t, n, r, i, a) {
			var o = t;
			e && (e = typeof e == "string" ? e : L.getPath(e), o = t ? A.join2(e, t) : e);
			var s = Be(r, i), c = L.create(o, s);
			if (n) {
				n = ze(n), L.chmod(c, s | 146);
				var l = L.open(c, 577);
				L.write(l, n, 0, n.length, 0, a), L.close(l), L.chmod(c, s);
			}
		},
		createDevice(e, t, n, r) {
			var i = A.join2(typeof e == "string" ? e : L.getPath(e), t), a = Be(!!n, !!r);
			L.createDevice.major ??= 64;
			var o = L.makedev(L.createDevice.major++, 0);
			return L.registerDevice(o, {
				open(e) {
					e.seekable = !1;
				},
				close(e) {
					r?.buffer?.length && r(10);
				},
				read(e, t, r, i, a) {
					for (var o = 0, s = 0; s < i; s++) {
						var c;
						try {
							c = n();
						} catch {
							throw new L.ErrnoError(29);
						}
						if (c === void 0 && !o) throw new L.ErrnoError(6);
						if (c == null) break;
						o++, t[r + s] = c;
					}
					return o && (e.node.atime = Date.now()), o;
				},
				write(e, t, n, i, a) {
					for (var o = 0; o < i; o++) try {
						r(t[n + o]);
					} catch {
						throw new L.ErrnoError(29);
					}
					return i && (e.node.mtime = e.node.ctime = Date.now()), o;
				}
			}), L.mkdev(i, a, o);
		},
		forceLoadFile(e) {
			if (e.isDevice || e.isFolder || e.link || e.contents) return !0;
			if (globalThis.XMLHttpRequest) v("Lazy loading should have been performed (contents set) in createLazyFile, but it was not. Lazy loading only works in web workers. Use --embed-file or --preload-file in emcc on the main thread.");
			else try {
				e.contents = u(e.url);
			} catch {
				throw new L.ErrnoError(29);
			}
		},
		createLazyFile(e, t, n, i, a) {
			class o {
				lengthKnown = !1;
				chunks = [];
				get(e) {
					if (!(e > this.length - 1 || e < 0)) {
						var t = e % this.chunkSize, n = e / this.chunkSize | 0;
						return this.getter(n)[t];
					}
				}
				setDataGetter(e) {
					this.getter = e;
				}
				cacheLength() {
					var e = new XMLHttpRequest();
					e.open("HEAD", n, !1), e.send(null), e.status >= 200 && e.status < 300 || e.status === 304 || v(`Couldn't load ${n}. Status: ${e.status}`);
					var t = Number(e.getResponseHeader("Content-length")), r, i = (r = e.getResponseHeader("Accept-Ranges")) && r === "bytes", a = (r = e.getResponseHeader("Content-Encoding")) && r === "gzip", o = 1048576;
					i || (o = t);
					var s = (e, r) => {
						e > r && v(`invalid range (${e}, ${r}) or no bytes requested!`), r > t - 1 && v(`only ${t} bytes available! programmer error!`);
						var i = new XMLHttpRequest();
						return i.open("GET", n, !1), t !== o && i.setRequestHeader("Range", `bytes=${e}-${r}`), i.responseType = "arraybuffer", i.overrideMimeType && i.overrideMimeType("text/plain; charset=x-user-defined"), i.send(null), i.status >= 200 && i.status < 300 || i.status === 304 || v(`Couldn't load ${n}. Status: ${i.status}`), i.response === void 0 ? Pe(i.responseText ?? "", !0) : new Uint8Array(i.response || []);
					}, c = this;
					c.setDataGetter((e) => {
						var n = e * o, r = (e + 1) * o - 1;
						return r = Math.min(r, t - 1), c.chunks[e] === void 0 && (c.chunks[e] = s(n, r)), c.chunks[e] === void 0 && v("doXHR failed!"), c.chunks[e];
					}), (a || !t) && (o = t = 1, t = this.getter(0).length, o = t, d("LazyFiles on gzip forces download of the whole file when length is accessed")), this._length = t, this._chunkSize = o, this.lengthKnown = !0;
				}
				get length() {
					return this.lengthKnown || this.cacheLength(), this._length;
				}
				get chunkSize() {
					return this.lengthKnown || this.cacheLength(), this._chunkSize;
				}
			}
			if (globalThis.XMLHttpRequest) {
				r || v("Cannot do synchronous binary XHRs outside webworkers in modern browsers. Use --embed-file or --preload-file in emcc");
				var s = {
					isDevice: !1,
					contents: new o()
				};
			} else var s = {
				isDevice: !1,
				url: n
			};
			var c = L.createFile(e, t, s, i, a);
			s.contents ? c.contents = s.contents : s.url && (c.contents = null, c.url = s.url), Object.defineProperties(c, { usedBytes: { get: function() {
				return this.contents.length;
			} } });
			var l = {};
			for (let [e, t] of Object.entries(c.stream_ops)) l[e] = (...e) => (L.forceLoadFile(c), t(...e));
			function u(e, t, n, r, i) {
				var a = e.node.contents;
				if (i >= a.length) return 0;
				var o = Math.min(a.length - i, r);
				if (a.slice) for (var s = 0; s < o; s++) t[n + s] = a[i + s];
				else for (var s = 0; s < o; s++) t[n + s] = a.get(i + s);
				return o;
			}
			return l.read = (e, t, n, r, i) => (L.forceLoadFile(c), u(e, t, n, r, i)), l.mmap = (e, t, n, r, i) => {
				L.forceLoadFile(c);
				var a = Re(t);
				if (!a) throw new L.ErrnoError(48);
				return u(e, b, a, t, n), {
					ptr: a,
					allocated: !0
				};
			}, c.stream_ops = l, c;
		}
	}, et = (e, t, n) => e ? M(P, e, t, n) : "", R, z, B, V = {
		currentUmask: 18,
		calculateAt(e, t, n) {
			if (A.isAbs(t)) return t;
			var r = e === -100 ? L.cwd() : V.getStreamFromFD(e).path;
			if (t.length == 0) {
				if (!n) throw new L.ErrnoError(44);
				return r;
			}
			return r + "/" + t;
		},
		writeStat(e, t) {
			z[e / 4] = t.dev, z[(e + 4) / 4] = t.mode, O[(e + 8) / 8] = BigInt(t.nlink), z[(e + 16) / 4] = t.uid, z[(e + 20) / 4] = t.gid, z[(e + 24) / 4] = t.rdev, B[(e + 32) / 8] = BigInt(t.size), R[(e + 40) / 4] = 4096, R[(e + 44) / 4] = t.blocks;
			var n = t.atime.getTime(), r = t.mtime.getTime(), i = t.ctime.getTime();
			return B[(e + 48) / 8] = BigInt(Math.floor(n / 1e3)), O[(e + 56) / 8] = BigInt(n % 1e3 * 1e3 * 1e3), B[(e + 64) / 8] = BigInt(Math.floor(r / 1e3)), O[(e + 72) / 8] = BigInt(r % 1e3 * 1e3 * 1e3), B[(e + 80) / 8] = BigInt(Math.floor(i / 1e3)), O[(e + 88) / 8] = BigInt(i % 1e3 * 1e3 * 1e3), B[(e + 96) / 8] = BigInt(t.ino), 0;
		},
		writeStatFs(e, t) {
			z[(e + 8) / 4] = t.bsize, z[(e + 72) / 4] = t.bsize, B[(e + 16) / 8] = BigInt(t.blocks), B[(e + 24) / 8] = BigInt(t.bfree), B[(e + 32) / 8] = BigInt(t.bavail), B[(e + 40) / 8] = BigInt(t.files), B[(e + 48) / 8] = BigInt(t.ffree), z[(e + 56) / 4] = t.fsid, z[(e + 80) / 4] = t.flags, z[(e + 64) / 4] = t.namelen;
		},
		doMsync(e, t, n, r, i) {
			if (!L.isFile(t.node.mode)) throw new L.ErrnoError(43);
			if (r & 2) return 0;
			var a = P.subarray(e, e + n);
			L.msync(t, a, i, n, r);
		},
		getStreamFromFD(e) {
			return L.getStreamChecked(e);
		},
		varargs: void 0,
		getStr(e) {
			return et(e);
		}
	};
	function tt(e, t) {
		e = w(e);
		try {
			return e = V.getStr(e), L.chmod(e, t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function nt(e) {
		try {
			var t = V.getStreamFromFD(e);
			return L.dupStream(t).fd;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function rt(e, t, n, r) {
		t = w(t);
		try {
			if (t = V.getStr(t), t = V.calculateAt(e, t), n & -8) return -28;
			var i = L.lookupPath(t, { follow: !0 }).node;
			if (!i) return -44;
			var a = "";
			return n & 4 && (a += "r"), n & 2 && (a += "w"), n & 1 && (a += "x"), a && L.nodePermissions(i, a) ? -2 : 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function H(e, t, n, r) {
		n = w(n), r = w(r);
		try {
			if (isNaN(n) || isNaN(r)) return -22;
			if (t != 0) return -138;
			if (n < 0 || r < 0) return -28;
			if (!V.getStreamFromFD(e).seekable) return -70;
			var i = L.fstat(e).size, a = n + r;
			return a > i && L.ftruncate(e, a), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function it(e, t) {
		try {
			return L.fchmod(e, t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var at = () => {
		var e = Number(O[V.varargs / 8]);
		return V.varargs += 8, e;
	}, U = () => {
		var e = R[V.varargs / 4];
		return V.varargs += 4, e;
	}, ot;
	function st(e, t, n) {
		n = w(n), V.varargs = n;
		try {
			var r = V.getStreamFromFD(e);
			switch (t) {
				case 0:
					var i = U();
					if (i < 0) return -28;
					for (; L.streams[i];) i++;
					return L.dupStream(r, i).fd;
				case 1:
				case 2: return 0;
				case 3: return r.flags;
				case 4:
					var i = U(), a = 289792;
					return r.flags = r.flags & ~a | i & a, 0;
				case 5:
					var i = at(), o = 0;
					return ot[(i + o) / 2] = 2, 0;
				case 6:
				case 7: return 0;
			}
			return -28;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ct(e, t) {
		t = w(t);
		try {
			return V.writeStat(t, L.fstat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var lt = (e, t, n) => Ne(e, P, t, n);
	function ut(e, t) {
		e = w(e), t = w(t);
		try {
			if (!t) return -28;
			var n = L.cwd(), r = Me(n) + 1;
			return t < r ? -68 : (lt(n, e, t), r);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function dt(e, t, n) {
		t = w(t), n = w(n);
		try {
			var r = V.getStreamFromFD(e);
			r.getdents ||= L.readdir(r.path);
			for (var i = 280, a = 0, o = L.llseek(r, 0, 1), s = Math.floor(o / i), c = Math.min(r.getdents.length, s + Math.floor(n / i)), l = s; l < c; l++) {
				var u, d, f = r.getdents[l];
				if (f === ".") u = r.node.id, d = 4;
				else if (f === "..") u = L.lookupPath(r.path, { parent: !0 }).node.id, d = 4;
				else {
					var p;
					try {
						p = L.lookupNode(r.node, f);
					} catch (e) {
						if (e?.errno === 28) continue;
						throw e;
					}
					u = p.id, d = L.isChrdev(p.mode) ? 2 : L.isDir(p.mode) ? 4 : L.isLink(p.mode) ? 10 : 8;
				}
				B[(t + a) / 8] = BigInt(u), B[(t + a + 8) / 8] = BigInt((l + 1) * i), ot[(t + a + 16) / 2] = 280, b[t + a + 18] = d, lt(f, t + a + 19, 256), a += i;
			}
			return L.llseek(r, l * i, 0), a;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ft(e, t, n) {
		n = w(n), V.varargs = n;
		try {
			var r = V.getStreamFromFD(e);
			switch (t) {
				case 21509: return r.tty ? 0 : -59;
				case 21505:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tcgets) {
						var i = r.tty.ops.ioctl_tcgets(r), a = at();
						R[a / 4] = i.c_iflag || 0, R[(a + 4) / 4] = i.c_oflag || 0, R[(a + 8) / 4] = i.c_cflag || 0, R[(a + 12) / 4] = i.c_lflag || 0;
						for (var o = 0; o < 32; o++) b[a + o + 17] = i.c_cc[o] || 0;
						return 0;
					}
					return 0;
				case 21510:
				case 21511:
				case 21512: return r.tty ? 0 : -59;
				case 21506:
				case 21507:
				case 21508:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tcsets) {
						for (var a = at(), s = R[a / 4], c = R[(a + 4) / 4], l = R[(a + 8) / 4], u = R[(a + 12) / 4], d = [], o = 0; o < 32; o++) d.push(b[a + o + 17]);
						return r.tty.ops.ioctl_tcsets(r.tty, t, {
							c_iflag: s,
							c_oflag: c,
							c_cflag: l,
							c_lflag: u,
							c_cc: d
						});
					}
					return 0;
				case 21519:
					if (!r.tty) return -59;
					var a = at();
					return R[a / 4] = 0, 0;
				case 21520: return r.tty ? -28 : -59;
				case 21537:
				case 21531:
					var a = at();
					return L.ioctl(r, t, a);
				case 21523:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tiocgwinsz) {
						var f = r.tty.ops.ioctl_tiocgwinsz(r.tty), a = at();
						ot[a / 2] = f[0], ot[(a + 2) / 2] = f[1];
					}
					return 0;
				case 21524: return r.tty ? 0 : -59;
				case 21515: return r.tty ? 0 : -59;
				default: return -28;
			}
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function pt(e, t) {
		e = w(e), t = w(t);
		try {
			return e = V.getStr(e), V.writeStat(t, L.lstat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function mt(e, t, n, r) {
		t = w(t), n = w(n);
		try {
			t = V.getStr(t);
			var i = r & 256, a = r & 4096;
			return r &= -6401, t = V.calculateAt(e, t, a), V.writeStat(n, i ? L.lstat(t) : L.stat(t));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ht(e, t, n, r) {
		t = w(t), r = w(r), V.varargs = r;
		try {
			t = V.getStr(t), t = V.calculateAt(e, t);
			var i = r ? U() : 0;
			return n & 64 && (i &= ~V.currentUmask), L.open(t, n, i).fd;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function gt(e, t, n, r) {
		t = w(t), n = w(n), r = w(r);
		try {
			if (t = V.getStr(t), t = V.calculateAt(e, t), r <= 0) return -28;
			var i = L.readlink(t), a = Math.min(r, Me(i)), o = b[n + a];
			return lt(i, n, r + 1), b[n + a] = o, a;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function _t(e, t, n, r) {
		t = w(t), r = w(r);
		try {
			return t = V.getStr(t), r = V.getStr(r), t = V.calculateAt(e, t), r = V.calculateAt(n, r), L.rename(t, r), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function vt(e) {
		e = w(e);
		try {
			return e = V.getStr(e), L.rmdir(e), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function yt(e, t) {
		e = w(e), t = w(t);
		try {
			return e = V.getStr(e), V.writeStat(t, L.stat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function bt(e, t, n) {
		e = w(e), n = w(n);
		try {
			return e = V.getStr(e), n = V.getStr(n), n = V.calculateAt(t, n), L.symlink(e, n), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function xt(e, t, n) {
		t = w(t);
		try {
			if (t = V.getStr(t), t = V.calculateAt(e, t), !n) L.unlink(t);
			else if (n === 512) L.rmdir(t);
			else return -28;
			return 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var St = () => v(""), Ct = {}, wt = (e) => {
		for (; e.length;) {
			var t = e.pop();
			e.pop()(t);
		}
	};
	function Tt(e) {
		return this.fromWireType(Number(O[e / 8]));
	}
	var Et = {}, Dt = {}, Ot = {};
	class kt extends Error {
		constructor(e) {
			super(e), this.name = "InternalError";
		}
	}
	var At = (e) => {
		throw new kt(e);
	}, jt = (e, t, n) => {
		e.forEach((e) => Ot[e] = t);
		function r(t) {
			var r = n(t);
			r.length !== e.length && At("Mismatched type converter count");
			for (var i = 0; i < e.length; ++i) J(e[i], r[i]);
		}
		var i = Array(t.length), a = [], o = 0;
		for (let [e, n] of t.entries()) Dt.hasOwnProperty(n) ? i[e] = Dt[n] : (a.push(n), Et.hasOwnProperty(n) || (Et[n] = []), Et[n].push(() => {
			i[e] = Dt[n], ++o, o === a.length && r(i);
		}));
		a.length === 0 && r(i);
	}, W = function(e) {
		e = w(e);
		var t = Ct[e];
		delete Ct[e];
		var n = t.rawConstructor, r = t.rawDestructor, i = t.fields, a = i.map((e) => e.getterReturnType).concat(i.map((e) => e.setterArgumentType));
		jt([e], a, (e) => {
			var a = {};
			for (var [o, s] of i.entries()) {
				let t = e[o], n = s.getter, r = s.getterContext, c = e[o + i.length], l = s.setter, u = s.setterContext;
				a[s.fieldName] = {
					read: (e) => t.fromWireType(n(r, e)),
					write: (e, t) => {
						var n = [];
						l(u, e, c.toWireType(n, t)), wt(n);
					},
					optional: t.optional
				};
			}
			return [{
				name: t.name,
				fromWireType: (e) => {
					var t = {};
					for (var n in a) t[n] = a[n].read(e);
					return r(e), t;
				},
				toWireType: (e, t) => {
					for (var i in a) if (!(i in t) && !a[i].optional) throw TypeError(`Missing field: "${i}"`);
					var o = n();
					for (i in a) a[i].write(o, t[i]);
					return e !== null && e.push(r, o), o;
				},
				readValueFromPointer: Tt,
				destructorFunction: r
			}];
		});
	}, G = (e) => {
		for (var t = "";;) {
			var n = P[e++];
			if (!n) return t;
			t += String.fromCharCode(n);
		}
	};
	class K extends Error {
		constructor(e) {
			super(e), this.name = "BindingError";
		}
	}
	var q = (e) => {
		throw new K(e);
	};
	function Mt(e, t, n = {}) {
		var r = t.name;
		if (e || q(`type "${r}" must have a positive integer typeid pointer`), Dt.hasOwnProperty(e)) {
			if (n.ignoreDuplicateRegistrations) return;
			q(`Cannot register type '${r}' twice`);
		}
		if (Dt[e] = t, delete Ot[e], Et.hasOwnProperty(e)) {
			var i = Et[e];
			delete Et[e], i.forEach((e) => e());
		}
	}
	function J(e, t, n = {}) {
		return Mt(e, t, n);
	}
	var Nt, Pt = (e, t, n) => {
		switch (t) {
			case 1: return n ? (e) => b[e] : (e) => P[e];
			case 2: return n ? (e) => ot[e / 2] : (e) => Nt[e / 2];
			case 4: return n ? (e) => R[e / 4] : (e) => z[e / 4];
			case 8: return n ? (e) => B[e / 8] : (e) => O[e / 8];
			default: throw TypeError(`invalid integer width (${t}): ${e}`);
		}
	}, Ft = function(e, t, n, r, i) {
		e = w(e), t = w(t), n = w(n), t = G(t);
		let a = r === 0n, o = (e) => e;
		if (a) {
			let e = n * 8;
			o = (t) => typeof t == "number" ? t >>> 0 : BigInt.asUintN(e, t), i = o(i);
		}
		J(e, {
			name: t,
			fromWireType: o,
			toWireType: (e, t) => (typeof t == "number" && (t = BigInt(t)), t),
			readValueFromPointer: Pt(t, n, !a),
			destructorFunction: null
		});
	};
	function It(e, t, n, r) {
		e = w(e), t = w(t), t = G(t), J(e, {
			name: t,
			fromWireType: function(e) {
				return !!e;
			},
			toWireType: function(e, t) {
				return t ? n : r;
			},
			readValueFromPointer: function(e) {
				return this.fromWireType(P[e]);
			},
			destructorFunction: null
		});
	}
	var Lt = (e) => ({
		count: e.count,
		deleteScheduled: e.deleteScheduled,
		preservePointerOnDelete: e.preservePointerOnDelete,
		ptr: e.ptr,
		ptrType: e.ptrType,
		smartPtr: e.smartPtr,
		smartPtrType: e.smartPtrType
	}), Rt = (e) => {
		function t(e) {
			return e.$$.ptrType.registeredClass.name;
		}
		q(t(e) + " instance already deleted");
	}, zt = !1, Bt = (e) => {}, Vt = (e) => {
		e.smartPtr ? e.smartPtrType.rawDestructor(e.smartPtr) : e.ptrType.registeredClass.rawDestructor(e.ptr);
	}, Ht = (e) => {
		--e.count.value, e.count.value === 0 && Vt(e);
	}, Ut = (e) => globalThis.FinalizationRegistry ? (zt = new FinalizationRegistry((e) => {
		Ht(e.$$);
	}), Ut = (e) => {
		var t = e.$$;
		if (t.smartPtr) {
			var n = { $$: t };
			zt.register(e, n, e);
		}
		return e;
	}, Bt = (e) => zt.unregister(e), Ut(e)) : (Ut = (e) => e, e), Wt = [], Gt = () => {
		for (; Wt.length;) {
			var e = Wt.pop();
			e.$$.deleteScheduled = !1, e.delete();
		}
	}, Kt, qt = () => {
		let e = Jt.prototype;
		Object.assign(e, {
			isAliasOf(e) {
				if (!(this instanceof Jt) || !(e instanceof Jt)) return !1;
				var t = this.$$.ptrType.registeredClass, n = this.$$.ptr;
				e.$$ = e.$$;
				for (var r = e.$$.ptrType.registeredClass, i = e.$$.ptr; t.baseClass;) n = t.upcast(n), t = t.baseClass;
				for (; r.baseClass;) i = r.upcast(i), r = r.baseClass;
				return t === r && n === i;
			},
			clone() {
				if (this.$$.ptr || Rt(this), this.$$.preservePointerOnDelete) return this.$$.count.value += 1, this;
				var e = Ut(Object.create(Object.getPrototypeOf(this), { $$: { value: Lt(this.$$) } }));
				return e.$$.count.value += 1, e.$$.deleteScheduled = !1, e;
			},
			delete() {
				this.$$.ptr || Rt(this), this.$$.deleteScheduled && !this.$$.preservePointerOnDelete && q("Object already scheduled for deletion"), Bt(this), Ht(this.$$), this.$$.preservePointerOnDelete || (this.$$.smartPtr = void 0, this.$$.ptr = void 0);
			},
			isDeleted() {
				return !this.$$.ptr;
			},
			deleteLater() {
				return this.$$.ptr || Rt(this), this.$$.deleteScheduled && !this.$$.preservePointerOnDelete && q("Object already scheduled for deletion"), Wt.push(this), Wt.length === 1 && Kt && Kt(Gt), this.$$.deleteScheduled = !0, this;
			}
		});
		let t = Symbol.dispose;
		t && (e[t] = e.delete);
	};
	function Jt() {}
	var Yt = (e, t) => Object.defineProperty(t, "name", { value: e }), Xt = {}, Zt = (e, t, n) => {
		if (e[t].overloadTable === void 0) {
			var r = e[t];
			e[t] = function(...r) {
				return e[t].overloadTable.hasOwnProperty(r.length) || q(`Function '${n}' called with an invalid number of arguments (${r.length}) - expects one of (${e[t].overloadTable})!`), e[t].overloadTable[r.length].apply(this, r);
			}, e[t].overloadTable = [], e[t].overloadTable[r.argCount] = r;
		}
	}, Qt = (e, n, r) => {
		t.hasOwnProperty(e) ? ((r === void 0 || t[e].overloadTable !== void 0 && t[e].overloadTable[r] !== void 0) && q(`Cannot register public name '${e}' twice`), Zt(t, e, e), t[e].overloadTable.hasOwnProperty(r) && q(`Cannot register multiple overloads of a function with the same number of arguments (${r})!`), t[e].overloadTable[r] = n) : (t[e] = n, t[e].argCount = r);
	}, $t = 48, en = 57, tn = (e) => {
		e = e.replace(/[^a-zA-Z0-9_]/g, "$");
		var t = e.charCodeAt(0);
		return t >= $t && t <= en ? `_${e}` : e;
	};
	function nn(e, t, n, r, i, a, o, s) {
		this.name = e, this.constructor = t, this.instancePrototype = n, this.rawDestructor = r, this.baseClass = i, this.getActualType = a, this.upcast = o, this.downcast = s, this.pureVirtualFunctions = [];
	}
	var rn = (e, t, n) => {
		for (; t !== n;) t.upcast || q(`Expected null or instance of ${n.name}, got an instance of ${t.name}`), e = t.upcast(e), t = t.baseClass;
		return e;
	}, an = (e) => {
		if (e === null) return "null";
		var t = typeof e;
		return t === "object" || t === "array" || t === "function" ? e.toString() : "" + e;
	};
	function on(e, t) {
		if (t === null) return this.isReference && q(`null is not a valid ${this.name}`), 0;
		t.$$ || q(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || q(`Cannot pass deleted object as a pointer of type ${this.name}`);
		var n = t.$$.ptrType.registeredClass;
		return rn(t.$$.ptr, n, this.registeredClass);
	}
	function sn(e, t) {
		var n;
		if (t === null) return this.isReference && q(`null is not a valid ${this.name}`), this.isSmartPointer ? (n = this.rawConstructor(), e !== null && e.push(this.rawDestructor, n), n) : 0;
		(!t || !t.$$) && q(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || q(`Cannot pass deleted object as a pointer of type ${this.name}`), !this.isConst && t.$$.ptrType.isConst && q(`Cannot convert argument of type ${t.$$.smartPtrType ? t.$$.smartPtrType.name : t.$$.ptrType.name} to parameter type ${this.name}`);
		var r = t.$$.ptrType.registeredClass;
		if (n = rn(t.$$.ptr, r, this.registeredClass), this.isSmartPointer) switch (t.$$.smartPtr === void 0 && q("Passing raw pointer to smart pointer is illegal"), this.sharingPolicy) {
			case 0:
				t.$$.smartPtrType === this ? n = t.$$.smartPtr : q(`Cannot convert argument of type ${t.$$.smartPtrType ? t.$$.smartPtrType.name : t.$$.ptrType.name} to parameter type ${this.name}`);
				break;
			case 1:
				n = t.$$.smartPtr;
				break;
			case 2:
				if (t.$$.smartPtrType === this) n = t.$$.smartPtr;
				else {
					var i = t.clone();
					n = this.rawShare(n, Z.toHandle(() => i.delete())), e !== null && e.push(this.rawDestructor, n);
				}
				break;
			default: q("Unsupported sharing policy");
		}
		return n;
	}
	function cn(e, t) {
		if (t === null) return this.isReference && q(`null is not a valid ${this.name}`), 0;
		t.$$ || q(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || q(`Cannot pass deleted object as a pointer of type ${this.name}`), t.$$.ptrType.isConst && q(`Cannot convert argument of type ${t.$$.ptrType.name} to parameter type ${this.name}`);
		var n = t.$$.ptrType.registeredClass;
		return rn(t.$$.ptr, n, this.registeredClass);
	}
	var ln = (e, t, n) => {
		if (t === n) return e;
		if (n.baseClass === void 0) return null;
		var r = ln(e, t, n.baseClass);
		return r === null ? null : n.downcast(r);
	}, un = {}, dn = (e, t) => {
		for (t === void 0 && q("ptr should not be undefined"); e.baseClass;) t = e.upcast(t), e = e.baseClass;
		return t;
	}, fn = (e, t) => (t = dn(e, t), un[t]), pn = (e, t) => ((!t.ptrType || !t.ptr) && At("makeClassHandle requires ptr and ptrType"), !!t.smartPtrType != !!t.smartPtr && At("Both smartPtrType and smartPtr must be specified"), t.count = { value: 1 }, Ut(Object.create(e, { $$: {
		value: t,
		writable: !0
	} })));
	function mn(e) {
		e = w(e);
		var t = this.getPointee(e);
		if (!t) return this.destructor(e), null;
		var n = fn(this.registeredClass, t);
		if (n !== void 0) {
			if (n.$$.count.value === 0) return n.$$.ptr = t, n.$$.smartPtr = e, n.clone();
			var r = n.clone();
			return this.destructor(e), r;
		}
		function i() {
			return this.isSmartPointer ? pn(this.registeredClass.instancePrototype, {
				ptrType: this.pointeeType,
				ptr: t,
				smartPtrType: this,
				smartPtr: e
			}) : pn(this.registeredClass.instancePrototype, {
				ptrType: this,
				ptr: e
			});
		}
		var a = Xt[this.registeredClass.getActualType(t)];
		if (!a) return i.call(this);
		var o = this.isConst ? a.constPointerType : a.pointerType, s = ln(t, this.registeredClass, o.registeredClass);
		return s === null ? i.call(this) : this.isSmartPointer ? pn(o.registeredClass.instancePrototype, {
			ptrType: o,
			ptr: s,
			smartPtrType: this,
			smartPtr: e
		}) : pn(o.registeredClass.instancePrototype, {
			ptrType: o,
			ptr: s
		});
	}
	var hn = () => {
		Object.assign(gn.prototype, {
			getPointee(e) {
				return this.rawGetPointee && (e = this.rawGetPointee(e)), e;
			},
			destructor(e) {
				this.rawDestructor?.(e);
			},
			readValueFromPointer: Tt,
			fromWireType: mn
		});
	};
	function gn(e, t, n, r, i, a, o, s, c, l, u) {
		this.name = e, this.registeredClass = t, this.isReference = n, this.isConst = r, this.isSmartPointer = i, this.pointeeType = a, this.sharingPolicy = o, this.rawGetPointee = s, this.rawConstructor = c, this.rawShare = l, this.rawDestructor = u, !i && t.baseClass === void 0 ? r ? (this.toWireType = on, this.destructorFunction = null) : (this.toWireType = cn, this.destructorFunction = null) : this.toWireType = sn;
	}
	var _n = (e, n, r) => {
		t.hasOwnProperty(e) || At("Replacing nonexistent public symbol"), t[e].overloadTable !== void 0 && r !== void 0 ? t[e].overloadTable[r] = n : (t[e] = n, t[e].argCount = r);
	}, vn = (e, t, n = [], r = !1) => {
		for (var i = 1; i < e.length; ++i) e[i] == "p" && (n[i - 1] = BigInt(n[i - 1]));
		var a = T(t)(...n);
		function o(t) {
			return e[0] == "p" ? Number(t) : t;
		}
		return o(a);
	}, yn = (e, t, n = !1) => (...r) => vn(e, t, r, n), Y = (e, t, n = !1) => {
		e = G(e);
		function r() {
			return e.includes("p") ? yn(e, t, n) : T(t);
		}
		var i = r();
		return typeof i != "function" && q(`unknown function pointer with signature ${e}: ${t}`), i;
	};
	class bn extends Error {}
	var xn = (e) => {
		var t = yi(e), n = G(t);
		return Q(t), n;
	}, Sn = (e, t) => {
		var n = [], r = {};
		function i(e) {
			if (!r[e] && !Dt[e]) {
				if (Ot[e]) {
					Ot[e].forEach(i);
					return;
				}
				n.push(e), r[e] = !0;
			}
		}
		throw t.forEach(i), new bn(`${e}: ` + n.map(xn).join([", "]));
	};
	function Cn(e, t, n, r, i, a, o, s, c, l, u, d, f) {
		e = w(e), t = w(t), n = w(n), r = w(r), i = w(i), a = w(a), o = w(o), s = w(s), c = w(c), l = w(l), u = w(u), d = w(d), f = w(f), u = G(u), a = Y(i, a), s &&= Y(o, s), l &&= Y(c, l), f = Y(d, f);
		var p = tn(u);
		Qt(p, function() {
			Sn(`Cannot construct ${u} due to unbound types`, [r]);
		}), jt([
			e,
			t,
			n
		], r ? [r] : [], (t) => {
			t = t[0];
			var n, i;
			r ? (n = t.registeredClass, i = n.instancePrototype) : i = Jt.prototype;
			var o = Yt(u, function(...e) {
				if (Object.getPrototypeOf(this) !== c) throw new K(`Use 'new' to construct ${u}`);
				if (d.constructor_body === void 0) throw new K(`${u} has no accessible constructor`);
				var t = d.constructor_body[e.length];
				if (t === void 0) throw new K(`Tried to invoke ctor of ${u} with invalid number of parameters (${e.length}) - expected (${Object.keys(d.constructor_body).toString()}) parameters instead!`);
				return t.apply(this, e);
			}), c = Object.create(i, { constructor: { value: o } });
			o.prototype = c;
			var d = new nn(u, o, c, f, n, a, s, l);
			d.baseClass && (d.baseClass.__derivedClasses ??= [], d.baseClass.__derivedClasses.push(d));
			var m = new gn(u, d, !0, !1, !1), h = new gn(u + "*", d, !1, !1, !1), g = new gn(u + " const*", d, !1, !0, !1);
			return Xt[e] = {
				pointerType: h,
				constPointerType: g
			}, _n(p, o), [
				m,
				h,
				g
			];
		});
	}
	var wn = [], X = [
		0,
		1,
		,
		1,
		null,
		1,
		!0,
		1,
		!1,
		1
	], Tn = [];
	function En(e) {
		if (e = w(e), e > 9 && --X[e + 1] === 0) {
			var t = X[e];
			X[e] = void 0;
			var n = Tn[e];
			n && (Tn[e] = void 0, n(t)), wn.push(e);
		}
	}
	var Z = {
		toValue: (e) => (e || q(`Cannot use deleted val. handle = ${e}`), X[e]),
		toHandle: (e) => {
			switch (e) {
				case void 0: return 2;
				case null: return 4;
				case !0: return 6;
				case !1: return 8;
				default: {
					let t = wn.pop() || X.length;
					return X[t] = e, X[t + 1] = 1, t;
				}
			}
		}
	}, Dn = {
		name: "emscripten::val",
		fromWireType: (e) => {
			var t = Z.toValue(e);
			return En(e), t;
		},
		toWireType: (e, t) => Z.toHandle(t),
		readValueFromPointer: Tt,
		destructorFunction: null
	};
	function On(e) {
		return e = w(e), J(e, Dn);
	}
	var kn = (e, t, n) => {
		switch (t) {
			case 1: return n ? function(e) {
				return this.fromWireType(b[e]);
			} : function(e) {
				return this.fromWireType(P[e]);
			};
			case 2: return n ? function(e) {
				return this.fromWireType(ot[e / 2]);
			} : function(e) {
				return this.fromWireType(Nt[e / 2]);
			};
			case 4: return n ? function(e) {
				return this.fromWireType(R[e / 4]);
			} : function(e) {
				return this.fromWireType(z[e / 4]);
			};
			default: throw TypeError(`invalid integer width (${t}): ${e}`);
		}
	};
	function An(e) {
		return e ? e === 1 ? "number" : "string" : "object";
	}
	function jn(e, n, r, i, a) {
		e = w(e), n = w(n), r = w(r), n = G(n);
		let o = An(a);
		switch (o) {
			case "object": {
				function t() {}
				t.values = {}, J(e, {
					name: n,
					constructor: t,
					valueType: o,
					fromWireType: function(e) {
						return this.constructor.values[e];
					},
					toWireType: (e, t) => t.value,
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, t);
				break;
			}
			case "number":
				var s = {};
				J(e, {
					name: n,
					keysMap: s,
					valueType: o,
					fromWireType: (e) => e,
					toWireType: (e, t) => t,
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, s), delete t[n].argCount;
				break;
			case "string":
				var c = {}, l = {}, s = {};
				J(e, {
					name: n,
					valuesMap: c,
					reverseMap: l,
					keysMap: s,
					valueType: o,
					fromWireType: function(e) {
						return this.reverseMap[e];
					},
					toWireType: function(e, t) {
						return this.valuesMap[t];
					},
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, s), delete t[n].argCount;
		}
	}
	var Mn = (e, t) => {
		var n = Dt[e];
		return n === void 0 && q(`${t} has unknown type ${xn(e)}`), n;
	};
	function Nn(e, t, n) {
		e = w(e), t = w(t);
		var r = Mn(e, "enum");
		switch (t = G(t), r.valueType) {
			case "object":
				var i = r.constructor, a = Object.create(r.constructor.prototype, {
					value: { value: n },
					constructor: { value: Yt(`${r.name}_${t}`, function() {}) }
				});
				i.values[n] = a, i[t] = a;
				break;
			case "number":
				r.keysMap[t] = n;
				break;
			case "string": r.valuesMap[t] = n, r.reverseMap[n] = t, r.keysMap[t] = t;
		}
	}
	var Pn, Fn, In = (e, t) => {
		switch (t) {
			case 4: return function(e) {
				return this.fromWireType(Pn[e / 4]);
			};
			case 8: return function(e) {
				return this.fromWireType(Fn[e / 8]);
			};
			default: throw TypeError(`invalid float width (${t}): ${e}`);
		}
	}, Ln = function(e, t, n) {
		e = w(e), t = w(t), n = w(n), t = G(t), J(e, {
			name: t,
			fromWireType: (e) => e,
			toWireType: (e, t) => t,
			readValueFromPointer: In(t, n),
			destructorFunction: null
		});
	};
	function Rn(e) {
		for (var t = 1; t < e.length; ++t) if (e[t] !== null && e[t].destructorFunction === void 0) return !0;
		return !1;
	}
	var zn = {
		ftf: function(e, t, n, r, i, a, o) {
			return function() {
				return a(n(r));
			};
		},
		ftft: function(e, t, n, r, i, a, o, s, c) {
			return function(e) {
				var t = s(null, e), i = n(r, t);
				return c(t), a(i);
			};
		},
		ftfn: function(e, t, n, r, i, a, o, s) {
			return function(e) {
				return a(n(r, s(null, e)));
			};
		},
		ftfnn: function(e, t, n, r, i, a, o, s, c) {
			return function(e, t) {
				return a(n(r, s(null, e), c(null, t)));
			};
		},
		fffn: function(e, t, n, r, i, a, o, s) {
			return function(e) {
				n(r, s(null, e));
			};
		},
		ftfnnn: function(e, t, n, r, i, a, o, s, c, l) {
			return function(e, t, i) {
				return a(n(r, s(null, e), c(null, t), l(null, i)));
			};
		},
		ftfnt: function(e, t, n, r, i, a, o, s, c, l) {
			return function(e, t) {
				var i = s(null, e), o = c(null, t), u = n(r, i, o);
				return l(o), a(u);
			};
		}
	};
	function Bn(e, t, n, r) {
		let i = [
			t ? "t" : "f",
			n ? "t" : "f",
			r ? "t" : "f"
		];
		for (let n = t ? 1 : 2; n < e.length; ++n) {
			let t = e[n], r = "";
			r = t.destructorFunction === void 0 ? "u" : t.destructorFunction === null ? "n" : "t", i.push(r);
		}
		return i.join("");
	}
	function Vn(e, t, n, r, i, a) {
		var o = t.length;
		o < 2 && q("argTypes array size mismatch! Must at least get return value and receiver (this) types!");
		for (var s = t[1] !== null && n !== null, c = Rn(t), l = !t[0].isVoid, u = t[0], d = t[1], f = [
			e,
			q,
			r,
			i,
			wt,
			u.fromWireType.bind(u),
			d?.toWireType.bind(d)
		], p = 2; p < o; ++p) {
			var m = t[p];
			f.push(m.toWireType.bind(m));
		}
		if (!c) for (var p = s ? 1 : 2; p < t.length; ++p) t[p].destructorFunction !== null && f.push(t[p].destructorFunction);
		return Yt(e, zn[Bn(t, s, l, a)](...f));
	}
	var Hn = (e, t) => {
		for (var n = [], r = 0; r < e; r++) n.push(Number(O[(t + r * 8) / 8]));
		return n;
	}, Un = (e) => {
		e = e.trim();
		let t = e.indexOf("(");
		return t === -1 ? e : e.slice(0, t);
	};
	function Wn(e, t, n, r, i, a, o, s) {
		e = w(e), n = w(n), r = w(r), i = w(i), a = w(a);
		var c = Hn(t, n);
		e = G(e), e = Un(e), i = Y(r, i, o), Qt(e, function() {
			Sn(`Cannot call ${e} due to unbound types`, c);
		}, t - 1), jt([], c, (n) => {
			var r = [n[0], null].concat(n.slice(1));
			return _n(e, Vn(e, r, null, i, a, o), t - 1), [];
		});
	}
	var Gn = function(e, t, n, r, i) {
		e = w(e), t = w(t), n = w(n), t = G(t);
		let a = r === 0, o = (e) => e;
		if (a) {
			var s = 32 - 8 * n;
			o = (e) => e << s >>> s, i = o(i);
		}
		J(e, {
			name: t,
			fromWireType: o,
			toWireType: (e, t) => t,
			readValueFromPointer: Pt(t, n, r !== 0),
			destructorFunction: null
		});
	};
	function Kn(e, t, n) {
		e = w(e), n = w(n);
		var r = [
			Int8Array,
			Uint8Array,
			Int16Array,
			Uint16Array,
			Int32Array,
			Uint32Array,
			Float32Array,
			Float64Array,
			BigInt64Array,
			BigUint64Array
		][t];
		function i(e) {
			var t = Number(O[e / 8]), n = Number(O[(e + 8) / 8]);
			return new r(b.buffer, n, t);
		}
		n = G(n), J(e, {
			name: n,
			fromWireType: i,
			readValueFromPointer: i
		}, { ignoreDuplicateRegistrations: !0 });
	}
	function qn(e, t) {
		e = w(e), t = w(t), t = G(t);
		var n = !0;
		J(e, {
			name: t,
			fromWireType(e) {
				var t = Number(O[e / 8]), r = e + 8, i;
				if (n) i = et(r, t, !0);
				else {
					i = "";
					for (var a = 0; a < t; ++a) i += String.fromCharCode(P[r + a]);
				}
				return Q(e), i;
			},
			toWireType(e, t) {
				t instanceof ArrayBuffer && (t = new Uint8Array(t));
				var r, i = typeof t == "string";
				i || ArrayBuffer.isView(t) && t.BYTES_PER_ELEMENT == 1 || q("Cannot pass non-string to std::string"), r = n && i ? Me(t) : t.length;
				var a = bi(8 + r + 1), o = a + 8;
				if (O[a / 8] = BigInt(r), i) {
					if (n) lt(t, o, r + 1);
					else for (var s = 0; s < r; ++s) {
						var c = t.charCodeAt(s);
						c > 255 && (Q(a), q("String has UTF-16 code units that do not fit in 8 bits")), P[o + s] = c;
					}
				} else P.set(t, o);
				return e !== null && e.push(Q, a), a;
			},
			readValueFromPointer: Tt,
			destructorFunction(e) {
				Q(e);
			}
		});
	}
	var Jn = globalThis.TextDecoder ? new TextDecoder("utf-16le") : void 0, Yn = (e, t, n) => {
		var r = e / 2, i = Ae(Nt, r, t / 2, n);
		if (i - r > 16 && Jn) return Jn.decode(Nt.subarray(r, i));
		for (var a = "", o = r; o < i; ++o) {
			var s = Nt[o];
			a += String.fromCharCode(s);
		}
		return a;
	}, Xn = (e, t, n = 2147483647) => {
		if (n < 2) return 0;
		n -= 2;
		for (var r = t, i = n < e.length * 2 ? n / 2 : e.length, a = 0; a < i; ++a) {
			var o = e.charCodeAt(a);
			ot[t / 2] = o, t += 2;
		}
		return ot[t / 2] = 0, t - r;
	}, Zn = (e) => e.length * 2, Qn = (e, t, n) => {
		for (var r = "", i = e / 4, a = 0; !(a >= t / 4); a++) {
			var o = z[i + a];
			if (!o && !n) break;
			r += String.fromCodePoint(o);
		}
		return r;
	}, $n = (e, t, n = 2147483647) => {
		if (n < 4) return 0;
		for (var r = t, i = r + n - 4, a = 0; a < e.length; ++a) {
			var o = e.codePointAt(a);
			if (o > 65535 && a++, R[t / 4] = o, t += 4, t + 4 > i) break;
		}
		return R[t / 4] = 0, t - r;
	}, er = (e) => {
		for (var t = 0, n = 0; n < e.length; ++n) e.codePointAt(n) > 65535 && n++, t += 4;
		return t;
	};
	function tr(e, t, n) {
		e = w(e), t = w(t), n = w(n), n = G(n);
		var r, i, a;
		t === 2 ? (r = Yn, i = Xn, a = Zn) : (r = Qn, i = $n, a = er), J(e, {
			name: n,
			fromWireType: (e) => {
				var n = Number(O[e / 8]), i = r(e + 8, n * t, !0);
				return Q(e), i;
			},
			toWireType: (e, r) => {
				typeof r != "string" && q(`Cannot pass non-string to C++ string type ${n}`);
				var o = a(r), s = bi(8 + o + t);
				return O[s / 8] = BigInt(o / t), i(r, s + 8, o + t), e !== null && e.push(Q, s), s;
			},
			readValueFromPointer: Tt,
			destructorFunction(e) {
				Q(e);
			}
		});
	}
	function nr(e, t, n, r, i, a) {
		e = w(e), t = w(t), n = w(n), r = w(r), i = w(i), a = w(a), Ct[e] = {
			name: G(t),
			rawConstructor: Y(n, r),
			rawDestructor: Y(i, a),
			fields: []
		};
	}
	function rr(e, t, n, r, i, a, o, s, c, l) {
		e = w(e), t = w(t), n = w(n), r = w(r), i = w(i), a = w(a), o = w(o), s = w(s), c = w(c), l = w(l), Ct[e].fields.push({
			fieldName: G(t),
			getterReturnType: n,
			getter: Y(r, i),
			getterContext: a,
			setterArgumentType: o,
			setter: Y(s, c),
			setterContext: l
		});
	}
	var ir = function(e, t) {
		e = w(e), t = w(t), t = G(t), J(e, {
			isVoid: !0,
			name: t,
			fromWireType: () => void 0,
			toWireType: (e, t) => void 0
		});
	}, ar = () => {};
	function or(e) {
		return e = w(e), e ? -52 : 0;
	}
	var sr = () => {
		throw new ee();
	}, cr = [], lr = (e) => {
		var t = cr.length;
		return cr.push(e), t;
	}, ur = (e, t) => {
		for (var n = Array(e), r = 0; r < e; ++r) n[r] = Mn(Number(O[(t + r * 8) / 8]), `parameter ${r}`);
		return n;
	}, dr = (e, t, n) => {
		var r = [], i = e(r, n);
		return r.length && (O[t / 8] = BigInt(Z.toHandle(r))), i;
	}, fr = {}, pr = (e) => {
		var t = fr[e];
		return t === void 0 ? G(e) : t;
	}, mr = function(e, t, n) {
		t = w(t);
		var r = (() => {
			var [r, ...i] = ur(e, t), a = r.toWireType.bind(r), o = i.map((e) => e.readValueFromPointer.bind(e));
			e--;
			var s = Array(e);
			return lr(Yt(`methodCaller<(${i.map((e) => e.name)}) => ${r.name}>`, (t, r, i, c) => {
				for (var l = 0, u = 0; u < e; ++u) s[u] = o[u](c + l), l += 16;
				var d;
				switch (n) {
					case 0:
						d = Z.toValue(t).apply(null, s);
						break;
					case 2:
						d = Reflect.construct(Z.toValue(t), s);
						break;
					case 3:
						d = s[0];
						break;
					case 1: d = Z.toValue(t)[pr(r)](...s);
				}
				return dr(a, i, d);
			}));
		})();
		return BigInt(r);
	};
	function hr(e) {
		e = w(e), e > 9 && (X[e + 1] += 1);
	}
	function gr(e, t, n, r, i) {
		return e = w(e), t = w(t), n = w(n), r = w(r), i = w(i), cr[e](t, n, r, i);
	}
	var _r = () => BigInt(Z.toHandle([])), vr = (e) => (e = w(e), BigInt(Z.toHandle(pr(e)))), yr = () => BigInt(Z.toHandle({}));
	function br(e) {
		e = w(e), wt(Z.toValue(e)), En(e);
	}
	function xr(e, t, n) {
		e = w(e), t = w(t), n = w(n), e = Z.toValue(e), t = Z.toValue(t), n = Z.toValue(n), e[t] = n;
	}
	function Sr(e, t) {
		e = w(e), t = w(t);
		var n = /* @__PURE__ */ new Date(e * 1e3);
		if (isNaN(n.getTime())) return 1;
		R[t / 4] = n.getUTCSeconds(), R[(t + 4) / 4] = n.getUTCMinutes(), R[(t + 8) / 4] = n.getUTCHours(), R[(t + 12) / 4] = n.getUTCDate(), R[(t + 16) / 4] = n.getUTCMonth(), R[(t + 20) / 4] = n.getUTCFullYear() - 1900, R[(t + 24) / 4] = n.getUTCDay();
		var r = Date.UTC(n.getUTCFullYear(), 0, 1, 0, 0, 0, 0), i = (n.getTime() - r) / 864e5 | 0;
		return R[(t + 28) / 4] = i, 0;
	}
	var Cr = (e) => e % 4 == 0 && (e % 100 != 0 || e % 400 == 0), wr = [
		0,
		31,
		60,
		91,
		121,
		152,
		182,
		213,
		244,
		274,
		305,
		335
	], Tr = [
		0,
		31,
		59,
		90,
		120,
		151,
		181,
		212,
		243,
		273,
		304,
		334
	], Er = (e) => (Cr(e.getFullYear()) ? wr : Tr)[e.getMonth()] + e.getDate() - 1;
	function Dr(e, t) {
		e = w(e), t = w(t);
		var n = /* @__PURE__ */ new Date(e * 1e3);
		if (isNaN(n.getTime())) return 1;
		R[t / 4] = n.getSeconds(), R[(t + 4) / 4] = n.getMinutes(), R[(t + 8) / 4] = n.getHours(), R[(t + 12) / 4] = n.getDate(), R[(t + 16) / 4] = n.getMonth(), R[(t + 20) / 4] = n.getFullYear() - 1900, R[(t + 24) / 4] = n.getDay();
		var r = Er(n) | 0;
		R[(t + 28) / 4] = r, B[(t + 40) / 8] = BigInt(-(n.getTimezoneOffset() * 60));
		var i = new Date(n.getFullYear(), 0, 1), a = new Date(n.getFullYear(), 6, 1).getTimezoneOffset(), o = i.getTimezoneOffset(), s = (a != o && n.getTimezoneOffset() == Math.min(o, a)) | 0;
		return R[(t + 32) / 4] = s, 0;
	}
	var Or = function(e) {
		e = w(e);
		var t = (() => {
			var t = new Date(R[(e + 20) / 4] + 1900, R[(e + 16) / 4], R[(e + 12) / 4], R[(e + 8) / 4], R[(e + 4) / 4], R[e / 4], 0);
			if (isNaN(t.getTime())) return -1;
			var n = R[(e + 32) / 4], r = t.getTimezoneOffset(), i = new Date(t.getFullYear(), 0, 1), a = new Date(t.getFullYear(), 6, 1).getTimezoneOffset(), o = i.getTimezoneOffset(), s = Math.min(o, a);
			if (n < 0) n = Number(a != o && s == r);
			else if (n > 0 != (s == r)) {
				var c = n > 0 ? s : Math.max(o, a);
				if (t.setTime(t.getTime() + (c - r) * 6e4), isNaN(t.getTime())) return -1;
			}
			R[(e + 32) / 4] = n, R[(e + 24) / 4] = t.getDay();
			var l = Er(t) | 0;
			return R[(e + 28) / 4] = l, R[e / 4] = t.getSeconds(), R[(e + 4) / 4] = t.getMinutes(), R[(e + 8) / 4] = t.getHours(), R[(e + 12) / 4] = t.getDate(), R[(e + 16) / 4] = t.getMonth(), R[(e + 20) / 4] = t.getYear(), t.getTime() / 1e3;
		})();
		return BigInt(t);
	};
	function kr(e, t, n, r, i, a, o) {
		e = w(e), i = w(i), a = w(a), o = w(o);
		try {
			var s = V.getStreamFromFD(r), c = L.mmap(s, e, i, t, n), l = c.ptr;
			return R[a / 4] = c.allocated, O[o / 8] = BigInt(l), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function Ar(e, t, n, r, i, a) {
		e = w(e), t = w(t), a = w(a);
		try {
			var o = V.getStreamFromFD(i);
			n & 2 && V.doMsync(e, o, t, r, a);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var jr = function(e, t, n, r) {
		e = w(e), t = w(t), n = w(n), r = w(r);
		var i = (/* @__PURE__ */ new Date()).getFullYear(), a = new Date(i, 0, 1), o = new Date(i, 6, 1), s = a.getTimezoneOffset(), c = o.getTimezoneOffset(), l = Math.max(s, c);
		O[e / 8] = BigInt(l * 60), R[t / 4] = Number(s != c);
		var u = (e) => {
			var t = e >= 0 ? "-" : "+", n = Math.abs(e);
			return `UTC${t}${String(Math.floor(n / 60)).padStart(2, "0")}${String(n % 60).padStart(2, "0")}`;
		}, d = u(s), f = u(c);
		c < s ? (lt(d, n, 17), lt(f, r, 17)) : (lt(d, r, 17), lt(f, n, 17));
	}, Mr = () => performance.now(), Nr = () => Date.now(), Pr = 1, Fr = (e) => e >= 0 && e <= 3;
	function Ir(e, t, n) {
		if (t = w(t), n = w(n), !Fr(e)) return 28;
		var r;
		if (e === 0) r = Nr();
		else if (Pr) r = Mr();
		else return 52;
		var i = Math.round(r * 1e3 * 1e3);
		return B[n / 8] = BigInt(i), 0;
	}
	var Lr = () => 17179869184, Rr = () => BigInt(Lr()), zr = (e) => {
		var t = (e - ki.buffer.byteLength + 65535) / 65536 | 0;
		try {
			return ki.grow(BigInt(t)), ne(), 1;
		} catch {}
	};
	function Br(e) {
		e = w(e);
		var t = P.length, n = Lr();
		if (e > n) return !1;
		for (var r = 1; r <= 4; r *= 2) {
			var i = t * (1 + .2 / r);
			if (i = Math.min(i, e + 100663296), zr(Math.min(n, Le(Math.max(e, i), 65536)))) return !0;
		}
		return !1;
	}
	var Vr = {}, Hr = () => i, Ur = () => {
		if (!Ur.strings) {
			var e = {
				USER: "web_user",
				LOGNAME: "web_user",
				PATH: "/",
				PWD: "/",
				HOME: "/home/web_user",
				LANG: (globalThis.navigator?.language ?? "C").replace("-", "_") + ".UTF-8",
				_: Hr()
			};
			for (var t in Vr) Vr[t] === void 0 ? delete e[t] : e[t] = Vr[t];
			var n = [];
			for (var t in e) n.push(`${t}=${e[t]}`);
			Ur.strings = n;
		}
		return Ur.strings;
	};
	function Wr(e, t) {
		e = w(e), t = w(t);
		var n = 0, r = 0;
		for (var i of Ur()) {
			var a = t + n;
			O[(e + r) / 8] = BigInt(a), n += lt(i, a, Infinity) + 1, r += 8;
		}
		return 0;
	}
	function Gr(e, t) {
		e = w(e), t = w(t);
		var n = Ur();
		O[e / 8] = BigInt(n.length);
		var r = 0;
		for (var i of n) r += Me(i) + 1;
		return O[t / 8] = BigInt(r), 0;
	}
	var Kr = () => !0, qr = (e) => {
		Kr() || (m = !0), a(e, new de(e));
	}, Jr = (e, t) => {
		qr(e);
	};
	function Yr(e) {
		try {
			var t = V.getStreamFromFD(e);
			return L.close(t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function Xr(e, t) {
		t = w(t);
		try {
			var n = 0, r = 0, i = 0, a = V.getStreamFromFD(e), o = a.tty ? 2 : L.isDir(a.mode) ? 3 : L.isLink(a.mode) ? 7 : 4;
			return b[t] = o, ot[(t + 2) / 2] = i, B[(t + 8) / 8] = BigInt(n), B[(t + 16) / 8] = BigInt(r), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	var Zr = (e, t, n, r) => {
		for (var i = 0, a = 0; a < n; a++) {
			var o = Number(O[t / 8]), s = Number(O[(t + 8) / 8]);
			t += 16;
			try {
				var c = L.read(e, b, o, s, r);
			} catch (e) {
				if (i > 0 && e instanceof L.ErrnoError && (e.errno == 6 || e.errno == 6)) break;
				throw e;
			}
			if (c < 0) return -1;
			if (i += c, c < s) break;
			r !== void 0 && (r += c);
		}
		return i;
	};
	function Qr(e, t, n, r, i) {
		t = w(t), n = w(n), r = w(r), i = w(i);
		try {
			if (isNaN(r)) return 22;
			var a = Zr(V.getStreamFromFD(e), t, n, r);
			return O[i / 8] = BigInt(a), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function $r(e, t, n, r) {
		t = w(t), n = w(n), r = w(r);
		try {
			var i = Zr(V.getStreamFromFD(e), t, n);
			return O[r / 8] = BigInt(i), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ei(e, t, n, r) {
		t = w(t), r = w(r);
		try {
			if (isNaN(t)) return 22;
			var i = V.getStreamFromFD(e);
			return L.llseek(i, t, n), B[r / 8] = BigInt(i.position), i.getdents && !t && n === 0 && (i.getdents = null), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ti(e) {
		try {
			var t = V.getStreamFromFD(e);
			return t.stream_ops?.fsync?.(t);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	var ni = (e, t, n, r) => {
		if (n == 1) return L.write(e, b, Number(O[t / 8]), Number(O[(t + 8) / 8]), r);
		for (var i = 0, a = 0, o = t; a < n; a++, o += 16) i += Number(O[(o + 8) / 8]);
		for (var s = new Uint8Array(i), c = 0, a = 0; a < n; a++, t += 16) {
			var l = Number(O[t / 8]), u = Number(O[(t + 8) / 8]);
			s.set(P.subarray(l, l + u), c), c += u;
		}
		return L.write(e, s, 0, i, r);
	};
	function ri(e, t, n, r) {
		t = w(t), n = w(n), r = w(r);
		try {
			var i = ni(V.getStreamFromFD(e), t, n);
			return O[r / 8] = BigInt(i), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ii(e) {
		return e = w(e), e;
	}
	function ai(e, t) {
		return e = w(e), t = w(t), De(P.subarray(e, e + t));
	}
	var oi = (e, t) => {
		if (si) for (var n = e; n < e + t; n++) {
			var r = T(n);
			r && si.set(r, n);
		}
	}, si, ci = (e) => (si || (si = /* @__PURE__ */ new WeakMap(), oi(0, Number(Ai.length))), si.get(e) || 0), li = [], ui = () => li.length ? li.pop() : Ai.grow(1n), di = (e, t) => {
		Ai.set(BigInt(e), t), pe[e] = Ai.get(BigInt(e));
	}, fi = (e) => {
		let t = e.length;
		return [
			t % 128 | 128,
			t >> 7,
			...e
		];
	}, pi = {
		i: 127,
		p: 126,
		j: 126,
		f: 125,
		d: 124,
		e: 111
	}, mi = (e) => fi(Array.from(e, (e) => pi[e])), hi = (e, t) => {
		var n = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0, 1, ...fi([
			1,
			96,
			...mi(t.slice(1)),
			...mi(t[0] === "v" ? "" : t[0])
		]), 2, 7, 1, 1, 101, 1, 102, 0, 0, 7, 5, 1, 1, 102, 0, 0), r = new WebAssembly.Module(n);
		return new WebAssembly.Instance(r, { e: { f: e } }).exports.f;
	}, gi = (e, t) => {
		var n = ci(e);
		if (n) return n;
		var r = ui();
		try {
			di(r, e);
		} catch (n) {
			if (!(n instanceof TypeError)) throw n;
			di(r, hi(e, t));
		}
		return si.set(e, r), r;
	};
	function _i(e, t = "i8") {
		switch (t.endsWith("*") && (t = "*"), t) {
			case "i1": return b[e];
			case "i8": return b[e];
			case "i16": return ot[e / 2];
			case "i32": return R[e / 4];
			case "i64": return B[e / 8];
			case "float": return Pn[e / 4];
			case "double": return Fn[e / 8];
			case "*": return Number(O[e / 8]);
			default: v(`invalid type for getValue: ${t}`);
		}
	}
	function vi(e, t, n = "i8") {
		switch (n.endsWith("*") && (n = "*"), n) {
			case "i1":
				b[e] = t;
				break;
			case "i8":
				b[e] = t;
				break;
			case "i16":
				ot[e / 2] = t;
				break;
			case "i32":
				R[e / 4] = t;
				break;
			case "i64":
				B[e / 8] = BigInt(t);
				break;
			case "float":
				Pn[e / 4] = t;
				break;
			case "double":
				Fn[e / 8] = t;
				break;
			case "*":
				O[e / 8] = BigInt(t);
				break;
			default: v(`invalid type for setValue: ${n}`);
		}
	}
	L.createPreloadedFile = $e, L.preloadFile = Qe, L.staticInit(), qt(), hn(), t.wasmBinary && (p = t.wasmBinary), t.addFunction = gi, t.setValue = vi, t.getValue = _i, t.UTF8ToString = et, t.stringToUTF8 = lt, t.lengthBytesUTF8 = Me, t.FS = L;
	var yi, bi, Q, xi, $, Si, Ci, wi, Ti, Ei, Di, Oi, ki, Ai;
	function ji(e) {
		yi = e.$b, t._MagickColor_Create = e.ac, t._MagickColor_Dispose = e.bc, t._MagickColor_Count_Get = e.cc, t._MagickColor_Red_Get = e.dc, t._MagickColor_Red_Set = e.ec, t._MagickColor_Green_Get = e.fc, t._MagickColor_Green_Set = e.gc, t._MagickColor_Blue_Get = e.hc, t._MagickColor_Blue_Set = e.ic, t._MagickColor_Alpha_Get = e.jc, t._MagickColor_Alpha_Set = e.kc, t._MagickColor_Black_Get = e.lc, t._MagickColor_Black_Set = e.mc, t._MagickColor_IsCMYK_Get = e.nc, t._MagickColor_IsCMYK_Set = e.oc, t._MagickColor_Clone = e.pc, t._MagickColor_FuzzyEquals = e.qc, t._MagickColor_Initialize = e.rc, t._MagickColorCollection_Create = e.tc, t._MagickColorCollection_Dispose = e.uc, t._MagickColorCollection_Get = e.vc, t._MagickColorCollection_Set = e.wc, t._DrawingWand_Create = e.xc, t._DrawingWand_Dispose = e.yc, t._DrawingWand_Affine = e.zc, t._DrawingWand_Alpha = e.Ac, t._DrawingWand_Arc = e.Bc, t._DrawingWand_Bezier = e.Cc, t._DrawingWand_BorderColor = e.Dc, t._DrawingWand_Circle = e.Ec, t._DrawingWand_ClipPath = e.Fc, t._DrawingWand_ClipRule = e.Gc, t._DrawingWand_ClipUnits = e.Hc, t._DrawingWand_Color = e.Ic, t._DrawingWand_Composite = e.Jc, t._DrawingWand_Density = e.Kc, t._DrawingWand_Ellipse = e.Lc, t._DrawingWand_FillColor = e.Mc, t._DrawingWand_FillOpacity = e.Nc, t._DrawingWand_FillPatternUrl = e.Oc, t._DrawingWand_FillRule = e.Pc, t._DrawingWand_Font = e.Qc, t._DrawingWand_FontFamily = e.Rc, t._DrawingWand_FontPointSize = e.Sc, t._DrawingWand_FontTypeMetrics = e.Tc, t._TypeMetric_Create = e.Uc, t._DrawingWand_Gravity = e.Vc, t._DrawingWand_Line = e.Wc, t._DrawingWand_PathArcAbs = e.Xc, t._DrawingWand_PathArcRel = e.Yc, t._DrawingWand_PathClose = e.Zc, t._DrawingWand_PathCurveToAbs = e._c, t._DrawingWand_PathCurveToRel = e.$c, t._DrawingWand_PathFinish = e.ad, t._DrawingWand_PathLineToAbs = e.bd, t._DrawingWand_PathLineToHorizontalAbs = e.cd, t._DrawingWand_PathLineToHorizontalRel = e.dd, t._DrawingWand_PathLineToRel = e.ed, t._DrawingWand_PathLineToVerticalAbs = e.fd, t._DrawingWand_PathLineToVerticalRel = e.gd, t._DrawingWand_PathMoveToAbs = e.hd, t._DrawingWand_PathMoveToRel = e.id, t._DrawingWand_PathQuadraticCurveToAbs = e.jd, t._DrawingWand_PathQuadraticCurveToRel = e.kd, t._DrawingWand_PathSmoothCurveToAbs = e.ld, t._DrawingWand_PathSmoothCurveToRel = e.md, t._DrawingWand_PathSmoothQuadraticCurveToAbs = e.nd, t._DrawingWand_PathSmoothQuadraticCurveToRel = e.od, t._DrawingWand_PathStart = e.pd, t._DrawingWand_Point = e.qd, t._DrawingWand_Polygon = e.rd, t._DrawingWand_Polyline = e.sd, t._DrawingWand_PopClipPath = e.td, t._DrawingWand_PopGraphicContext = e.ud, t._DrawingWand_PopPattern = e.vd, t._DrawingWand_PushClipPath = e.wd, t._DrawingWand_PushGraphicContext = e.xd, t._DrawingWand_PushPattern = e.yd, t._DrawingWand_Rectangle = e.zd, t._DrawingWand_Render = e.Ad, t._DrawingWand_Rotation = e.Bd, t._DrawingWand_RoundRectangle = e.Cd, t._DrawingWand_Scaling = e.Dd, t._DrawingWand_SkewX = e.Ed, t._DrawingWand_SkewY = e.Fd, t._DrawingWand_StrokeAntialias = e.Gd, t._DrawingWand_StrokeColor = e.Hd, t._DrawingWand_StrokeDashArray = e.Id, t._DrawingWand_StrokeDashOffset = e.Jd, t._DrawingWand_StrokeLineCap = e.Kd, t._DrawingWand_StrokeLineJoin = e.Ld, t._DrawingWand_StrokeMiterLimit = e.Md, t._DrawingWand_StrokeOpacity = e.Nd, t._DrawingWand_StrokePatternUrl = e.Od, t._DrawingWand_StrokeWidth = e.Pd, t._DrawingWand_Text = e.Qd, t._DrawingWand_TextAlignment = e.Rd, t._DrawingWand_TextAntialias = e.Sd, t._DrawingWand_TextDecoration = e.Td, t._DrawingWand_TextDirection = e.Ud, t._DrawingWand_TextEncoding = e.Vd, t._DrawingWand_TextInterlineSpacing = e.Wd, t._DrawingWand_TextInterwordSpacing = e.Xd, t._DrawingWand_TextKerning = e.Yd, t._DrawingWand_TextUnderColor = e.Zd, t._DrawingWand_Translation = e._d, t._DrawingWand_Viewbox = e.$d, t._MagickExceptionHelper_Description = e.ae, t._MagickExceptionHelper_Dispose = e.be, t._MagickExceptionHelper_Related = e.ce, t._MagickExceptionHelper_RelatedCount = e.de, t._MagickExceptionHelper_Message = e.ee, t._MagickExceptionHelper_Severity = e.fe, t._PdfInfo_PageCount = e.ge, t._Environment_Initialize = e.he, t._Environment_GetEnv = e.ie, t._Environment_SetEnv = e.je, t._MagickMemory_Relinquish = e.ke, t._Magick_Delegates_Get = e.le, t._Magick_Features_Get = e.me, t._Magick_ImageMagickVersion_Get = e.ne, t._Magick_GetFonts = e.oe, t._Magick_GetFontFamily = e.pe, t._Magick_GetFontName = e.qe, t._Magick_GetWindowsResource = e.re, t._Magick_DisposeFonts = e.se, t._Magick_ResetRandomSeed = e.te, t._Magick_SetDefaultFontFile = e.ue, t._Magick_SetRandomSeed = e.ve, t._Magick_SetLogDelegate = e.we, t._Magick_SetLogEvents = e.xe, t._MagickFormatInfo_CreateList = e.ye, t._MagickFormatInfo_DisposeList = e.ze, t._MagickFormatInfo_CanReadMultithreaded_Get = e.Ae, t._MagickFormatInfo_CanWriteMultithreaded_Get = e.Be, t._MagickFormatInfo_Description_Get = e.Ce, t._MagickFormatInfo_Format_Get = e.De, t._MagickFormatInfo_MimeType_Get = e.Ee, t._MagickFormatInfo_Module_Get = e.Fe, t._MagickFormatInfo_SupportsMultipleFrames_Get = e.Ge, t._MagickFormatInfo_SupportsReading_Get = e.He, t._MagickFormatInfo_SupportsWriting_Get = e.Ie, t._MagickFormatInfo_Version_Get = e.Je, t._MagickFormatInfo_GetInfo = e.Ke, t._MagickFormatInfo_GetInfoByName = e.Le, t._MagickFormatInfo_GetInfoWithBlob = e.Me, t._MagickFormatInfo_Unregister = e.Ne, t._MagickImage_Create = e.Oe, t._MagickImage_Dispose = e.Pe, t._MagickImage_AnimationDelay_Get = e.Qe, t._MagickImage_AnimationDelay_Set = e.Re, t._MagickImage_AnimationIterations_Get = e.Se, t._MagickImage_AnimationIterations_Set = e.Te, t._MagickImage_AnimationTicksPerSecond_Get = e.Ue, t._MagickImage_AnimationTicksPerSecond_Set = e.Ve, t._MagickImage_BackgroundColor_Get = e.We, t._MagickImage_BackgroundColor_Set = e.Xe, t._MagickImage_BaseHeight_Get = e.Ye, t._MagickImage_BaseWidth_Get = e.Ze, t._MagickImage_BlackPointCompensation_Get = e._e, t._MagickImage_BlackPointCompensation_Set = e.$e, t._MagickImage_BorderColor_Get = e.af, t._MagickImage_BorderColor_Set = e.bf, t._MagickImage_BoundingBox_Get = e.cf, t._MagickRectangle_Create = e.df, t._MagickImage_ChannelCount_Get = e.ef, t._MagickImage_ChromaBlue_Get = e.ff, t._PrimaryInfo_Create = e.gf, t._MagickImage_ChromaBlue_Set = e.hf, t._MagickImage_ChromaGreen_Get = e.jf, t._MagickImage_ChromaGreen_Set = e.kf, t._MagickImage_ChromaRed_Get = e.lf, t._MagickImage_ChromaRed_Set = e.mf, t._MagickImage_ChromaWhite_Get = e.nf, t._MagickImage_ChromaWhite_Set = e.of, t._MagickImage_ClassType_Get = e.pf, t._MagickImage_ClassType_Set = e.qf, t._QuantizeSettings_Create = e.rf, t._QuantizeSettings_Dispose = e.sf, t._MagickImage_ColorFuzz_Get = e.tf, t._MagickImage_ColorFuzz_Set = e.uf, t._MagickImage_ColormapSize_Get = e.vf, t._MagickImage_ColormapSize_Set = e.wf, t._MagickImage_ColorSpace_Get = e.xf, t._MagickImage_ColorSpace_Set = e.yf, t._MagickImage_ColorType_Get = e.zf, t._MagickImage_ColorType_Set = e.Af, t._MagickImage_Compose_Get = e.Bf, t._MagickImage_Compose_Set = e.Cf, t._MagickImage_Compression_Get = e.Df, t._MagickImage_Compression_Set = e.Ef, t._MagickImage_Depth_Get = e.Ff, t._MagickImage_Depth_Set = e.Gf, t._MagickImage_EncodingGeometry_Get = e.Hf, t._MagickImage_Endian_Get = e.If, t._MagickImage_Endian_Set = e.Jf, t._MagickImage_FileName_Get = e.Kf, t._MagickImage_FileName_Set = e.Lf, t._MagickImage_FilterType_Get = e.Mf, t._MagickImage_FilterType_Set = e.Nf, t._MagickImage_Format_Get = e.Of, t._MagickImage_Format_Set = e.Pf, t._MagickImage_Gamma_Get = e.Qf, t._MagickImage_GifDisposeMethod_Get = e.Rf, t._MagickImage_GifDisposeMethod_Set = e.Sf, t._MagickImage_HasAlpha_Get = e.Tf, t._MagickImage_HasAlpha_Set = e.Uf, t._MagickImage_Height_Get = e.Vf, t._MagickImage_Interlace_Get = e.Wf, t._MagickImage_Interlace_Set = e.Xf, t._MagickImage_Interpolate_Get = e.Yf, t._MagickImage_Interpolate_Set = e.Zf, t._MagickImage_IsOpaque_Get = e._f, t._MagickImage_MatteColor_Get = e.$f, t._MagickImage_MatteColor_Set = e.ag, t._MagickImage_MeanErrorPerPixel_Get = e.bg, t._MagickImage_MetaChannelCount_Get = e.cg, t._MagickImage_MetaChannelCount_Set = e.dg, t._MagickImage_NormalizedMaximumError_Get = e.eg, t._MagickImage_NormalizedMeanError_Get = e.fg, t._MagickImage_Orientation_Get = e.gg, t._MagickImage_Orientation_Set = e.hg, t._MagickImage_Page_Get = e.ig, t._MagickImage_Page_Set = e.jg, t._MagickImage_Quality_Get = e.kg, t._MagickImage_Quality_Set = e.lg, t._MagickImage_RenderingIntent_Get = e.mg, t._MagickImage_RenderingIntent_Set = e.ng, t._MagickImage_ResolutionUnits_Get = e.og, t._MagickImage_ResolutionUnits_Set = e.pg, t._MagickImage_ResolutionX_Get = e.qg, t._MagickImage_ResolutionX_Set = e.rg, t._MagickImage_ResolutionY_Get = e.sg, t._MagickImage_ResolutionY_Set = e.tg, t._MagickImage_Signature_Get = e.ug, t._MagickImage_TotalColors_Get = e.vg, t._MagickImage_VirtualPixelMethod_Get = e.wg, t._MagickImage_VirtualPixelMethod_Set = e.xg, t._MagickImage_Width_Get = e.yg, t._MagickImage_AdaptiveBlur = e.zg, t._MagickImage_AdaptiveResize = e.Ag, t._MagickImage_AdaptiveSharpen = e.Bg, t._MagickImage_AdaptiveThreshold = e.Cg, t._MagickImage_AddNoise = e.Dg, t._MagickImage_AffineTransform = e.Eg, t._MagickImage_Annotate = e.Fg, t._MagickImage_AutoGamma = e.Gg, t._MagickImage_AutoLevel = e.Hg, t._MagickImage_AutoOrient = e.Ig, t._MagickImage_AutoThreshold = e.Jg, t._MagickImage_BilateralBlur = e.Kg, t._MagickImage_BlackThreshold = e.Lg, t._MagickImage_BlueShift = e.Mg, t._MagickImage_Blur = e.Ng, t._MagickImage_Border = e.Og, t._MagickImage_BrightnessContrast = e.Pg, t._MagickImage_CannyEdge = e.Qg, t._MagickImage_ChannelOffset = e.Rg, t._MagickImage_Charcoal = e.Sg, t._MagickImage_Chop = e.Tg, t._MagickImage_Clahe = e.Ug, t._MagickImage_Clamp = e.Vg, t._MagickImage_ClipPath = e.Wg, t._MagickImage_Clone = e.Xg, t._MagickImage_CloneArea = e.Yg, t._MagickImage_Clut = e.Zg, t._MagickImage_ColorDecisionList = e._g, t._MagickImage_Colorize = e.$g, t._MagickImage_ColorMatrix = e.ah, t._MagickImage_ColorThreshold = e.bh, t._MagickImage_Compare = e.ch, t._MagickImage_CompareDistortion = e.dh, t._MagickImage_Composite = e.eh, t._MagickImage_CompositeGravity = e.fh, t._MagickImage_ConnectedComponents = e.gh, t._MagickImage_Contrast = e.hh, t._MagickImage_ContrastStretch = e.ih, t._MagickImage_ConvexHull = e.jh, t._MagickImage_Convolve = e.kh, t._MagickImage_CopyPixels = e.lh, t._MagickImage_Crop = e.mh, t._MagickImage_CropToTiles = e.nh, t._MagickImage_CycleColormap = e.oh, t._MagickImage_Decipher = e.ph, t._MagickImage_Deskew = e.qh, t._MagickImage_Despeckle = e.rh, t._MagickImage_DetermineBitDepth = e.sh, t._MagickImage_DetermineColorType = e.th, t._MagickImage_Distort = e.uh, t._MagickImage_Edge = e.vh, t._MagickImage_Emboss = e.wh, t._MagickImage_Encipher = e.xh, t._MagickImage_Enhance = e.yh, t._MagickImage_Equalize = e.zh, t._MagickImage_Equals = e.Ah, t._MagickImage_EvaluateFunction = e.Bh, t._MagickImage_EvaluateGeometry = e.Ch, t._MagickImage_EvaluateOperator = e.Dh, t._MagickImage_Extent = e.Eh, t._MagickImage_Flip = e.Fh, t._MagickImage_FloodFill = e.Gh, t._MagickImage_Flop = e.Hh, t._MagickImage_FontTypeMetrics = e.Ih, t._MagickImage_FormatExpression = e.Jh, t._MagickImage_Frame = e.Kh, t._MagickImage_Fx = e.Lh, t._MagickImage_GammaCorrect = e.Mh, t._MagickImage_GaussianBlur = e.Nh, t._MagickImage_GetArtifact = e.Oh, t._MagickImage_GetAttribute = e.Ph, t._MagickImage_GetColormapColor = e.Qh, t._MagickImage_GetNext = e.Rh, t._MagickImage_GetNextArtifactName = e.Sh, t._MagickImage_GetNextAttributeName = e.Th, t._MagickImage_GetNextProfileName = e.Uh, t._MagickImage_GetProfile = e.Vh, t._MagickImage_GetReadMask = e.Wh, t._MagickImage_GetWriteMask = e.Xh, t._MagickImage_Grayscale = e.Yh, t._MagickImage_HaldClut = e.Zh, t._MagickImage_HasChannel = e._h, t._MagickImage_HasProfile = e.$h, t._MagickImage_Histogram = e.ai, t._MagickImage_HoughLine = e.bi, t._MagickImage_Implode = e.ci, t._MagickImage_ImportIndexedPixels = e.di, t._MagickImage_ImportPixels = e.ei, t._MagickImage_Integral = e.fi, t._MagickImage_InterpolativeResize = e.gi, t._MagickImage_InverseLevel = e.hi, t._MagickImage_Kmeans = e.ii, t._MagickImage_Kuwahara = e.ji, t._MagickImage_Level = e.ki, t._MagickImage_LevelColors = e.li, t._MagickImage_LinearStretch = e.mi, t._MagickImage_LiquidRescale = e.ni, t._MagickImage_LocalContrast = e.oi, t._MagickImage_Magnify = e.pi, t._MagickImage_MeanShift = e.qi, t._MagickImage_Minify = e.ri, t._MagickImage_MinimumBoundingBox = e.si, t._MagickImage_Modulate = e.ti, t._MagickImage_Moments = e.ui, t._MagickImage_Morphology = e.vi, t._MagickImage_MotionBlur = e.wi, t._MagickImage_Negate = e.xi, t._MagickImage_Normalize = e.yi, t._MagickImage_OilPaint = e.zi, t._MagickImage_Opaque = e.Ai, t._MagickImage_OrderedDither = e.Bi, t._MagickImage_Perceptible = e.Ci, t._MagickImage_PerceptualHash = e.Di, t._MagickImage_Quantize = e.Ei, t._MagickImage_Polaroid = e.Fi, t._MagickImage_Posterize = e.Gi, t._MagickImage_RaiseOrLower = e.Hi, t._MagickImage_RandomThreshold = e.Ii, t._MagickImage_RangeThreshold = e.Ji, t._MagickImage_ReadBlob = e.Ki, t._MagickImage_ReadFile = e.Li, t._MagickImage_ReadPixels = e.Mi, t._MagickImage_ReadStream = e.Ni, t._MagickImage_RegionMask = e.Oi, t._MagickImage_Remap = e.Pi, t._MagickImage_RemoveArtifact = e.Qi, t._MagickImage_RemoveAttribute = e.Ri, t._MagickImage_RemoveProfile = e.Si, t._MagickImage_ResetArtifactIterator = e.Ti, t._MagickImage_ResetAttributeIterator = e.Ui, t._MagickImage_ResetProfileIterator = e.Vi, t._MagickImage_Resample = e.Wi, t._MagickImage_Resize = e.Xi, t._MagickImage_Roll = e.Yi, t._MagickImage_Rotate = e.Zi, t._MagickImage_RotationalBlur = e._i, t._MagickImage_Sample = e.$i, t._MagickImage_Scale = e.aj, t._MagickImage_Segment = e.bj, t._MagickImage_SelectiveBlur = e.cj, t._MagickImage_Separate = e.dj, t._MagickImage_SepiaTone = e.ej, t._MagickImage_SetAlpha = e.fj, t._MagickImage_SetArtifact = e.gj, t._MagickImage_SetAttribute = e.hj, t._MagickImage_SetBitDepth = e.ij, t._MagickImage_SetClientData = e.jj, t._MagickImage_SetColormapColor = e.kj, t._MagickImage_SetColorMetric = e.lj, t._MagickImage_SetNext = e.mj, t._MagickImage_SetProfile = e.nj, t._MagickImage_SetProgressDelegate = e.oj, t._MagickImage_SetReadMask = e.pj, t._MagickImage_SetWriteMask = e.qj, t._MagickImage_Shade = e.rj, t._MagickImage_Shadow = e.sj, t._MagickImage_Sharpen = e.tj, t._MagickImage_Shave = e.uj, t._MagickImage_Shear = e.vj, t._MagickImage_SigmoidalContrast = e.wj, t._MagickImage_SparseColor = e.xj, t._MagickImage_Spread = e.yj, t._MagickImage_Sketch = e.zj, t._MagickImage_Solarize = e.Aj, t._MagickImage_SortPixels = e.Bj, t._MagickImage_Splice = e.Cj, t._MagickImage_Statistic = e.Dj, t._MagickImage_Statistics = e.Ej, t._MagickImage_Stegano = e.Fj, t._MagickImage_Stereo = e.Gj, t._MagickImage_Strip = e.Hj, t._MagickImage_SubImageSearch = e.Ij, t._MagickImage_Swirl = e.Jj, t._MagickImage_Texture = e.Kj, t._MagickImage_Threshold = e.Lj, t._MagickImage_Thumbnail = e.Mj, t._MagickImage_Tint = e.Nj, t._MagickImage_Transparent = e.Oj, t._MagickImage_TransparentChroma = e.Pj, t._MagickImage_Transpose = e.Qj, t._MagickImage_Transverse = e.Rj, t._MagickImage_Trim = e.Sj, t._MagickImage_UniqueColors = e.Tj, t._MagickImage_UnsharpMask = e.Uj, t._MagickImage_Vignette = e.Vj, t._MagickImage_Wave = e.Wj, t._MagickImage_WaveletDenoise = e.Xj, t._MagickImage_WhiteBalance = e.Yj, t._MagickImage_WhiteThreshold = e.Zj, t._MagickImage_WriteBlob = e._j, t._MagickImage_WriteFile = e.$j, t._MagickImage_WriteStream = e.ak, t._MagickImageCollection_Append = e.bk, t._MagickImageCollection_Coalesce = e.ck, t._MagickImageCollection_Combine = e.dk, t._MagickImageCollection_Complex = e.ek, t._MagickImageCollection_Deconstruct = e.fk, t._MagickImageCollection_Dispose = e.gk, t._MagickImageCollection_Evaluate = e.hk, t._MagickImageCollection_Fx = e.ik, t._MagickImageCollection_Merge = e.jk, t._MagickImageCollection_Montage = e.kk, t._MagickImageCollection_Morph = e.lk, t._MagickImageCollection_Optimize = e.mk, t._MagickImageCollection_OptimizePlus = e.nk, t._MagickImageCollection_OptimizeTransparency = e.ok, t._MagickImageCollection_Polynomial = e.pk, t._MagickImageCollection_Quantize = e.qk, t._MagickImageCollection_ReadBlob = e.rk, t._MagickImageCollection_ReadFile = e.sk, t._MagickImageCollection_ReadStream = e.tk, t._MagickImageCollection_Remap = e.uk, t._MagickImageCollection_Smush = e.vk, t._MagickImageCollection_WriteFile = e.wk, t._MagickImageCollection_WriteStream = e.xk, t._DoubleMatrix_Create = e.yk, t._DoubleMatrix_Dispose = e.zk, t._OpenCL_GetDevices = e.Ak, t._OpenCL_GetDevice = e.Bk, t._OpenCL_GetEnabled = e.Ck, t._OpenCL_SetEnabled = e.Dk, t._OpenCLDevice_DeviceType_Get = e.Ek, t._OpenCLDevice_BenchmarkScore_Get = e.Fk, t._OpenCLDevice_IsEnabled_Get = e.Gk, t._OpenCLDevice_IsEnabled_Set = e.Hk, t._OpenCLDevice_Name_Get = e.Ik, t._OpenCLDevice_Version_Get = e.Jk, t._OpenCLDevice_GetKernelProfileRecords = e.Kk, t._OpenCLDevice_GetKernelProfileRecord = e.Lk, t._OpenCLDevice_SetProfileKernels = e.Mk, t._OpenCLKernelProfileRecord_Count_Get = e.Nk, t._OpenCLKernelProfileRecord_Name_Get = e.Ok, t._OpenCLKernelProfileRecord_MaximumDuration_Get = e.Pk, t._OpenCLKernelProfileRecord_MinimumDuration_Get = e.Qk, t._OpenCLKernelProfileRecord_TotalDuration_Get = e.Rk, t._JpegOptimizer_CompressFile = e.Sk, t._JpegOptimizer_CompressStream = e.Tk, bi = t._malloc = e.Uk, Q = t._free = e.Vk, t._PixelCollection_Create = e.Wk, t._PixelCollection_Dispose = e.Xk, t._PixelCollection_GetArea = e.Yk, t._PixelCollection_GetReadOnlyArea = e.Zk, t._PixelCollection_SetArea = e._k, t._PixelCollection_ToByteArray = e.$k, t._PixelCollection_ToShortArray = e.al, t._Quantum_Depth_Get = e.bl, t._Quantum_Max_Get = e.cl, t._ResourceLimits_Area_Get = e.dl, t._ResourceLimits_Area_Set = e.el, t._ResourceLimits_Disk_Get = e.fl, t._ResourceLimits_Disk_Set = e.gl, t._ResourceLimits_Height_Get = e.hl, t._ResourceLimits_Height_Set = e.il, t._ResourceLimits_ListLength_Get = e.jl, t._ResourceLimits_ListLength_Set = e.kl, t._ResourceLimits_MaxMemoryRequest_Get = e.ll, t._ResourceLimits_MaxMemoryRequest_Set = e.ml, t._ResourceLimits_MaxProfileSize_Get = e.nl, t._ResourceLimits_MaxProfileSize_Set = e.ol, t._ResourceLimits_Memory_Get = e.pl, t._ResourceLimits_Memory_Set = e.ql, t._ResourceLimits_Thread_Get = e.rl, t._ResourceLimits_Thread_Set = e.sl, t._ResourceLimits_Throttle_Get = e.tl, t._ResourceLimits_Throttle_Set = e.ul, t._ResourceLimits_Time_Get = e.vl, t._ResourceLimits_Time_Set = e.wl, t._ResourceLimits_Width_Get = e.xl, t._ResourceLimits_Width_Set = e.yl, t._ResourceLimits_LimitMemory = e.zl, t._ResourceLimits_TrimMemory = e.Al, t._DrawingSettings_Create = e.Bl, t._DrawingSettings_Dispose = e.Cl, t._DrawingSettings_BorderColor_Get = e.Dl, t._DrawingSettings_BorderColor_Set = e.El, t._DrawingSettings_FillColor_Get = e.Fl, t._DrawingSettings_FillColor_Set = e.Gl, t._DrawingSettings_FillRule_Get = e.Hl, t._DrawingSettings_FillRule_Set = e.Il, t._DrawingSettings_Font_Get = e.Jl, t._DrawingSettings_Font_Set = e.Kl, t._DrawingSettings_FontFamily_Get = e.Ll, t._DrawingSettings_FontFamily_Set = e.Ml, t._DrawingSettings_FontPointsize_Get = e.Nl, t._DrawingSettings_FontPointsize_Set = e.Ol, t._DrawingSettings_FontStyle_Get = e.Pl, t._DrawingSettings_FontStyle_Set = e.Ql, t._DrawingSettings_FontWeight_Get = e.Rl, t._DrawingSettings_FontWeight_Set = e.Sl, t._DrawingSettings_StrokeAntiAlias_Get = e.Tl, t._DrawingSettings_StrokeAntiAlias_Set = e.Ul, t._DrawingSettings_StrokeColor_Get = e.Vl, t._DrawingSettings_StrokeColor_Set = e.Wl, t._DrawingSettings_StrokeDashOffset_Get = e.Xl, t._DrawingSettings_StrokeDashOffset_Set = e.Yl, t._DrawingSettings_StrokeLineCap_Get = e.Zl, t._DrawingSettings_StrokeLineCap_Set = e._l, t._DrawingSettings_StrokeLineJoin_Get = e.$l, t._DrawingSettings_StrokeLineJoin_Set = e.am, t._DrawingSettings_StrokeMiterLimit_Get = e.bm, t._DrawingSettings_StrokeMiterLimit_Set = e.cm, t._DrawingSettings_StrokeWidth_Get = e.dm, t._DrawingSettings_StrokeWidth_Set = e.em, t._DrawingSettings_TextAntiAlias_Get = e.fm, t._DrawingSettings_TextAntiAlias_Set = e.gm, t._DrawingSettings_TextDirection_Get = e.hm, t._DrawingSettings_TextDirection_Set = e.im, t._DrawingSettings_TextEncoding_Get = e.jm, t._DrawingSettings_TextEncoding_Set = e.km, t._DrawingSettings_TextGravity_Get = e.lm, t._DrawingSettings_TextGravity_Set = e.mm, t._DrawingSettings_TextInterlineSpacing_Get = e.nm, t._DrawingSettings_TextInterlineSpacing_Set = e.om, t._DrawingSettings_TextInterwordSpacing_Get = e.pm, t._DrawingSettings_TextInterwordSpacing_Set = e.qm, t._DrawingSettings_TextKerning_Get = e.rm, t._DrawingSettings_TextKerning_Set = e.sm, t._DrawingSettings_TextUnderColor_Get = e.tm, t._DrawingSettings_TextUnderColor_Set = e.um, t._DrawingSettings_SetAffine = e.vm, t._DrawingSettings_SetFillPattern = e.wm, t._DrawingSettings_SetStrokeDashArray = e.xm, t._DrawingSettings_SetStrokePattern = e.ym, t._DrawingSettings_SetText = e.zm, t._MagickSettings_Create = e.Am, t._MagickSettings_Dispose = e.Bm, t._MagickSettings_AntiAlias_Get = e.Cm, t._MagickSettings_AntiAlias_Set = e.Dm, t._MagickSettings_BackgroundColor_Get = e.Em, t._MagickSettings_BackgroundColor_Set = e.Fm, t._MagickSettings_ColorSpace_Get = e.Gm, t._MagickSettings_ColorSpace_Set = e.Hm, t._MagickSettings_ColorType_Get = e.Im, t._MagickSettings_ColorType_Set = e.Jm, t._MagickSettings_Compression_Get = e.Km, t._MagickSettings_Compression_Set = e.Lm, t._MagickSettings_Debug_Get = e.Mm, t._MagickSettings_Debug_Set = e.Nm, t._MagickSettings_Density_Get = e.Om, t._MagickSettings_Density_Set = e.Pm, t._MagickSettings_Depth_Get = e.Qm, t._MagickSettings_Depth_Set = e.Rm, t._MagickSettings_Endian_Get = e.Sm, t._MagickSettings_Endian_Set = e.Tm, t._MagickSettings_Extract_Get = e.Um, t._MagickSettings_Extract_Set = e.Vm, t._MagickSettings_Format_Get = e.Wm, t._MagickSettings_Format_Set = e.Xm, t._MagickSettings_FontPointsize_Get = e.Ym, t._MagickSettings_FontPointsize_Set = e.Zm, t._MagickSettings_Interlace_Get = e._m, t._MagickSettings_Interlace_Set = e.$m, t._MagickSettings_Monochrome_Get = e.an, t._MagickSettings_Monochrome_Set = e.bn, t._MagickSettings_Verbose_Get = e.cn, t._MagickSettings_Verbose_Set = e.dn, t._MagickSettings_SetColorFuzz = e.en, t._MagickSettings_SetFileName = e.fn, t._MagickSettings_SetFont = e.gn, t._MagickSettings_SetNumberScenes = e.hn, t._MagickSettings_SetOption = e.jn, t._MagickSettings_SetPage = e.kn, t._MagickSettings_SetPing = e.ln, t._MagickSettings_SetQuality = e.mn, t._MagickSettings_SetScenes = e.nn, t._MagickSettings_SetScene = e.on, t._MagickSettings_SetSize = e.pn, t._MontageSettings_Create = e.qn, t._MontageSettings_Dispose = e.rn, t._MontageSettings_SetBackgroundColor = e.sn, t._MontageSettings_SetBorderColor = e.tn, t._MontageSettings_SetBorderWidth = e.un, t._MontageSettings_SetFillColor = e.vn, t._MontageSettings_SetFont = e.wn, t._MontageSettings_SetFontPointsize = e.xn, t._MontageSettings_SetFrameGeometry = e.yn, t._MontageSettings_SetGeometry = e.zn, t._MontageSettings_SetGravity = e.An, t._MontageSettings_SetShadow = e.Bn, t._MontageSettings_SetStrokeColor = e.Cn, t._MontageSettings_SetTextureFileName = e.Dn, t._MontageSettings_SetTileGeometry = e.En, t._MontageSettings_SetTitle = e.Fn, t._QuantizeSettings_SetColors = e.Gn, t._QuantizeSettings_SetColorSpace = e.Hn, t._QuantizeSettings_SetDitherMethod = e.In, t._QuantizeSettings_SetMeasureErrors = e.Jn, t._QuantizeSettings_SetTreeDepth = e.Kn, t._ChannelMoments_Centroid_Get = e.Ln, t._ChannelMoments_EllipseAngle_Get = e.Mn, t._ChannelMoments_EllipseAxis_Get = e.Nn, t._ChannelMoments_EllipseEccentricity_Get = e.On, t._ChannelMoments_EllipseIntensity_Get = e.Pn, t._ChannelMoments_GetHuInvariants = e.Qn, t._ChannelPerceptualHash_GetHuPhash = e.Rn, t._ChannelStatistics_Depth_Get = e.Sn, t._ChannelStatistics_Entropy_Get = e.Tn, t._ChannelStatistics_Kurtosis_Get = e.Un, t._ChannelStatistics_Maximum_Get = e.Vn, t._ChannelStatistics_Mean_Get = e.Wn, t._ChannelStatistics_Minimum_Get = e.Xn, t._ChannelStatistics_Skewness_Get = e.Yn, t._ChannelStatistics_StandardDeviation_Get = e.Zn, t._Moments_DisposeList = e._n, t._Moments_GetInstance = e.$n, t._PerceptualHash_DisposeList = e.ao, t._PerceptualHash_GetInstance = e.bo, t._Statistics_DisposeList = e.co, t._Statistics_GetInstance = e.eo, t._ConnectedComponent_DisposeList = e.fo, t._ConnectedComponent_GetArea = e.go, t._ConnectedComponent_GetCentroid = e.ho, t._ConnectedComponent_GetColor = e.io, t._ConnectedComponent_GetHeight = e.jo, t._ConnectedComponent_GetId = e.ko, t._ConnectedComponent_GetWidth = e.lo, t._ConnectedComponent_GetX = e.mo, t._ConnectedComponent_GetY = e.no, t._ConnectedComponent_GetInstance = e.oo, t._MagickGeometry_Create = e.po, t._MagickGeometry_Dispose = e.qo, t._MagickGeometry_X_Get = e.ro, t._MagickGeometry_Y_Get = e.so, t._MagickGeometry_Width_Get = e.to, t._MagickGeometry_Height_Get = e.uo, t._MagickGeometry_Initialize = e.vo, t._MagickRectangle_Dispose = e.wo, t._MagickRectangle_X_Get = e.xo, t._MagickRectangle_X_Set = e.yo, t._MagickRectangle_Y_Get = e.zo, t._MagickRectangle_Y_Set = e.Ao, t._MagickRectangle_Width_Get = e.Bo, t._MagickRectangle_Width_Set = e.Co, t._MagickRectangle_Height_Get = e.Do, t._MagickRectangle_Height_Set = e.Eo, t._MagickRectangle_FromPageSize = e.Fo, t._OffsetInfo_Create = e.Go, t._OffsetInfo_Dispose = e.Ho, t._OffsetInfo_SetX = e.Io, t._OffsetInfo_SetY = e.Jo, t._PointInfo_X_Get = e.Ko, t._PointInfo_Y_Get = e.Lo, t._PointInfoCollection_Create = e.Mo, t._PointInfoCollection_Dispose = e.No, t._PointInfoCollection_GetX = e.Oo, t._PointInfoCollection_GetY = e.Po, t._PointInfoCollection_Set = e.Qo, t._PrimaryInfo_Dispose = e.Ro, t._PrimaryInfo_X_Get = e.So, t._PrimaryInfo_X_Set = e.To, t._PrimaryInfo_Y_Get = e.Uo, t._PrimaryInfo_Y_Set = e.Vo, t._PrimaryInfo_Z_Get = e.Wo, t._PrimaryInfo_Z_Set = e.Xo, t._StringInfo_Length_Get = e.Yo, t._StringInfo_Datum_Get = e.Zo, t._TypeMetric_Dispose = e._o, t._TypeMetric_Ascent_Get = e.$o, t._TypeMetric_Descent_Get = e.ap, t._TypeMetric_MaxHorizontalAdvance_Get = e.bp, t._TypeMetric_TextHeight_Get = e.cp, t._TypeMetric_TextWidth_Get = e.dp, t._TypeMetric_UnderlinePosition_Get = e.ep, t._TypeMetric_UnderlineThickness_Get = e.fp, xi = e.gp, $ = e.hp, Si = e.ip, Ci = e.jp, wi = e.kp, Ti = e.lp, Ei = e.mp, Di = e.np, Oi = e.op, ki = e.Zb, Ai = e.sc;
	}
	var Mi = {
		wb: E,
		z: ge,
		E: _e,
		e: k,
		k: xe,
		La: Se,
		va: Ce,
		a: we,
		mb: Te,
		i: Ee,
		Rb: tt,
		Pb: nt,
		Sb: rt,
		zb: H,
		Ob: it,
		ua: st,
		Nb: ct,
		Ib: ut,
		vb: dt,
		Vb: ft,
		Kb: pt,
		Lb: mt,
		sa: ht,
		ub: gt,
		sb: _t,
		tb: vt,
		Mb: yt,
		rb: bt,
		Ha: xt,
		Tb: St,
		$a: W,
		aa: Ft,
		Ya: It,
		na: Cn,
		Wa: On,
		O: jn,
		h: Nn,
		xa: Ln,
		w: Wn,
		V: Gn,
		D: Kn,
		Xa: qn,
		ka: tr,
		ab: nr,
		ma: rr,
		Za: ir,
		xb: ar,
		pb: or,
		nb: sr,
		H: mr,
		C: En,
		_a: hr,
		G: gr,
		la: _r,
		Q: vr,
		ba: yr,
		F: br,
		N: xr,
		Cb: Sr,
		Db: Dr,
		Eb: Or,
		Ab: kr,
		Bb: Ar,
		Fb: jr,
		Qb: Ir,
		Ja: Nr,
		qb: Rr,
		ra: Mr,
		ob: Br,
		Wb: Wr,
		Xb: Gr,
		X: Jr,
		_: Yr,
		Ia: Xr,
		yb: Qr,
		Ka: $r,
		Gb: ei,
		Jb: ti,
		ta: ri,
		ib: aa,
		Ea: Ya,
		Fa: Ja,
		hb: sa,
		Ca: na,
		b: Qi,
		c: Ri,
		p: Fi,
		ja: Na,
		Ua: ka,
		J: $i,
		g: Ii,
		v: Wi,
		Ra: Fa,
		qa: Ki,
		ga: Qa,
		t: ea,
		n: Xi,
		y: Ji,
		fb: pa,
		gb: fa,
		Qa: La,
		A: Zi,
		M: da,
		Sa: Pa,
		d: Li,
		Yb: Wa,
		W: Pi,
		j: Gi,
		jb: qi,
		r: oa,
		kb: Ka,
		lb: Ga,
		S: ia,
		K: Sa,
		q: ta,
		T: Za,
		B: Yi,
		Ga: qa,
		Aa: la,
		o: Ma,
		fa: eo,
		Ba: ra,
		oa: Ea,
		Da: $a,
		l: zi,
		s: Ui,
		L: Hi,
		Y: Oa,
		ia: Ia,
		x: Ni,
		I: Vi,
		da: ca,
		ha: Ua,
		m: ja,
		f: Bi,
		za: ma,
		ya: ha,
		P: ba,
		ca: ya,
		eb: ga,
		Ma: Ha,
		db: Ca,
		Ta: Aa,
		u: ua,
		Z: _a,
		$: Ra,
		pa: va,
		cb: wa,
		Oa: Ba,
		ea: to,
		Na: Va,
		R: xa,
		Pa: za,
		bb: Ta,
		Va: Da,
		U: Xa,
		wa: ii,
		Ub: qr,
		Hb: ai
	};
	function Ni(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Pi(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Fi(e, t, n) {
		var r = S();
		try {
			return T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ii(e, t, n) {
		var r = S();
		try {
			return T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Li(e, t) {
		var n = S();
		try {
			return T(Number(e))(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ri(e, t) {
		var n = S();
		try {
			return T(Number(e))(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function zi(e, t) {
		var n = S();
		try {
			T(Number(e))(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Bi(e, t, n) {
		var r = S();
		try {
			T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Vi(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Hi(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ui(e, t, n) {
		var r = S();
		try {
			T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Wi(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Gi(e, t, n) {
		var r = S();
		try {
			return T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ki(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function qi(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ji(e, t, n, r, i, a, o, s, c) {
		var l = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s, c);
		} catch (e) {
			if (x(l), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Yi(e, t, n, r, i, a, o) {
		var s = S();
		try {
			return T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Xi(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Zi(e) {
		var t = S();
		try {
			return T(Number(e))();
		} catch (e) {
			if (x(t), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Qi(e, t, n, r, i, a, o) {
		var s = S();
		try {
			return T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function $i(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ea(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ta(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function na(e, t, n) {
		var r = S();
		try {
			return T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ra(e, t, n) {
		var r = S();
		try {
			T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ia(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function aa(e, t, n) {
		var r = S();
		try {
			return T(Number(e))(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function oa(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function sa(e, t) {
		var n = S();
		try {
			return T(Number(e))(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ca(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function la(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function ua(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function da(e, t) {
		var n = S();
		try {
			return T(Number(e))(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function fa(e, t, n, r, i, a, o, s, c, l) {
		var u = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s, c, l);
		} catch (e) {
			if (x(u), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function pa(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ma(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ha(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ga(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function _a(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function va(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ya(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ba(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function xa(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Sa(e, t, n, r, i, a, o) {
		var s = S();
		try {
			return T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ca(e, t, n, r, i, a, o, s, c, l) {
		var u = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l);
		} catch (e) {
			if (x(u), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function wa(e, t, n, r, i, a, o) {
		var s = S();
		try {
			T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ta(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ea(e, t, n, r) {
		var i = S();
		try {
			T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Da(e, t, n, r, i, a, o, s, c, l) {
		var u = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l);
		} catch (e) {
			if (x(u), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Oa(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ka(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Aa(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ja(e, t, n, r, i) {
		var a = S();
		try {
			T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ma(e) {
		var t = S();
		try {
			T(Number(e))();
		} catch (e) {
			if (x(t), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Na(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Pa(e, t, n, r, i, a, o, s, c, l, u, d) {
		var f = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s, c, l, u, d);
		} catch (e) {
			if (x(f), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Fa(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ia(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function La(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ra(e, t, n, r, i, a) {
		var o = S();
		try {
			T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function za(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ba(e, t, n, r, i, a, o, s, c) {
		var l = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c);
		} catch (e) {
			if (x(l), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Va(e, t, n, r, i, a, o, s, c, l, u, d) {
		var f = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l, u, d);
		} catch (e) {
			if (x(f), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ha(e, t, n, r, i, a, o) {
		var s = S();
		try {
			T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ua(e, t, n, r, i, a, o) {
		var s = S();
		try {
			T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Wa(e, t, n, r, i) {
		var a = S();
		try {
			return T(Number(e))(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ga(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ka(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(Number(e))(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function qa(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ja(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ya(e, t, n, r) {
		var i = S();
		try {
			return T(Number(e))(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Xa(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Za(e, t, n, r, i, a, o) {
		var s = S();
		try {
			return T(Number(e))(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Qa(e, t, n, r, i, a, o, s, c, l, u, d) {
		var f = S();
		try {
			return T(Number(e))(t, n, r, i, a, o, s, c, l, u, d);
		} catch (e) {
			if (x(f), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function $a(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function eo(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function to(e, t, n, r, i, a, o, s, c, l, u, d, f, p, m, h) {
		var ee = S();
		try {
			T(Number(e))(t, n, r, i, a, o, s, c, l, u, d, f, p, m, h);
		} catch (e) {
			if (x(ee), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function no(e) {
		e = Object.assign({}, e);
		var t = (e) => (t) => Number(e(BigInt(t))), n = (e) => (t) => e(BigInt(t));
		return e.$b = t(e.$b), e.Uk = t(e.Uk), e.Vk = n(e.Vk), e.gp = ((e) => (t, n) => Number(e(BigInt(t), BigInt(n))))(e.gp), e.hp = ((e) => (t, n) => e(BigInt(t), n))(e.hp), e.jp = n(e.jp), e._emscripten_stack_alloc = t(e._emscripten_stack_alloc), e.kp = ((e) => () => Number(e()))(e.kp), e.lp = n(e.lp), e.mp = n(e.mp), e.np = ((e) => (t, n, r) => e(BigInt(t), BigInt(n), BigInt(r)))(e.np), e.op = t(e.op), e;
	}
	async function ro() {
		Ke && await Ge(), !m && re();
	}
	var io = await y();
	return await ro(), t;
}
//#endregion
//#region node_modules/@dlemstra/magick-native/x86/magick.js
async function L(e = {}) {
	var t = e, n = !!globalThis.window, r = !!globalThis.WorkerGlobalScope;
	globalThis.process?.versions?.node && globalThis.process?.type, (!globalThis.crypto || !globalThis.crypto.getRandomValues) && (globalThis.crypto = { getRandomValues: (e) => {
		for (let t = 0; t < e.length; t++) e[t] = Math.random() * 256 | 0;
	} }), t._CastToSize = (e) => e, t._NullPointer = 0, t._PointerSize = 4;
	var i = "./this.program", a = (e, t) => {
		throw t;
	}, o = {}.url, s = "";
	function c(e) {
		return t.locateFile ? t.locateFile(e, s) : s + e;
	}
	var l, u;
	if (n || r) {
		try {
			s = new URL(".", o).href;
		} catch {}
		r && (u = (e) => {
			var t = new XMLHttpRequest();
			return t.open("GET", e, !1), t.responseType = "arraybuffer", t.send(null), new Uint8Array(t.response);
		}), l = async (e) => {
			if (h(e)) return new Promise((t, n) => {
				var r = new XMLHttpRequest();
				r.open("GET", e, !0), r.responseType = "arraybuffer", r.onload = () => {
					if (r.status == 200 || r.status == 0 && r.response) {
						t(r.response);
						return;
					}
					n(r.status);
				}, r.onerror = n, r.send(null);
			});
			var t = await fetch(e, { credentials: "same-origin" });
			if (t.ok) return t.arrayBuffer();
			throw Error(t.status + " : " + t.url);
		};
	}
	var d = console.log.bind(console), f = console.error.bind(console), p, m = !1, h = (e) => e.startsWith("file://");
	class g {}
	class ee extends g {}
	class _ extends g {
		constructor(e) {
			super(), this.excPtr = e;
		}
	}
	function te() {
		return ki.buffer;
	}
	function ne() {
		if (!b?.buffer?.resizable) {
			var e = te();
			b = new Int8Array(e), U = new Int16Array(e), t.HEAPU8 = P = new Uint8Array(e), J = new Uint16Array(e), R = new Int32Array(e), O = new Uint32Array(e), Pn = new Float32Array(e), Fn = new Float64Array(e), z = new BigInt64Array(e), Nt = new BigUint64Array(e);
		}
	}
	function re() {
		!t.noFSInit && !L.initialized && L.init(), N.init(), Sa.qb(), L.ignorePermissions = !1;
	}
	function v(e) {
		throw e = `Aborted(${e})`, f(e), m = !0, e += ". Build with -sASSERTIONS for more info.", new WebAssembly.RuntimeError(e);
	}
	var ie;
	function ae() {
		return t.locateFile ? c("magick.wasm") : new URL("data:text/plain;base64,").href;
	}
	function oe(e) {
		if (e == ie && p) return new Uint8Array(p);
		if (u) return u(e);
		throw "both async and sync fetching of the wasm failed";
	}
	async function se(e) {
		if (!p) try {
			var t = await l(e);
			return new Uint8Array(t);
		} catch {}
		return oe(e);
	}
	async function ce(e, t) {
		try {
			var n = await se(e);
			return await WebAssembly.instantiate(n, t);
		} catch (e) {
			f(`failed to asynchronously prepare wasm: ${e}`), v(e);
		}
	}
	async function le(e, t, n) {
		if (!e && !h(t)) try {
			var r = fetch(t, { credentials: "same-origin" });
			return await WebAssembly.instantiateStreaming(r, n);
		} catch (e) {
			f(`wasm streaming compile failed: ${e}`), f("falling back to ArrayBuffer instantiation");
		}
		return ce(t, n);
	}
	function ue() {
		return { a: Mi };
	}
	async function y() {
		function e(e) {
			return Sa = e.exports, Sa = ba(Sa), ji(Sa), ne(), Sa;
		}
		function n(t) {
			return e(t.instance);
		}
		var r = ue(), i = t.instantiateWasm;
		return i ? new Promise((t) => {
			i(r, (n) => t(e(n)));
		}) : (ie ??= ae(), n(await le(p, ie, r)));
	}
	class de {
		name = "ExitStatus";
		constructor(e) {
			this.message = `Program terminated with exit(${e})`, this.status = e;
		}
	}
	var b, x = (e) => Ci(e), S = () => wi(), fe = 9007199254740992, C = -9007199254740992, w = (e) => e < C || e > fe ? NaN : Number(e), pe = [], T = (e) => {
		var t = pe[e];
		return t || (pe[e] = t = Ai.get(e)), t;
	};
	function E(e, t) {
		return e >>>= 0, T(e)(t);
	}
	var me = [], he = 0;
	function ge(e) {
		e >>>= 0;
		var t = new ve(e);
		return t.get_caught() || (t.set_caught(!0), he--), t.set_rethrown(!1), me.push(t), Oi(e);
	}
	var D = null, _e = () => {
		$(0, 0);
		var e = me.pop();
		Ti(e.excPtr), D = null;
	}, O;
	class ve {
		constructor(e) {
			this.excPtr = e, this.ptr = e - 24;
		}
		set_type(e) {
			O[this.ptr + 4 >>> 2 >>> 0] = e;
		}
		get_type() {
			return O[this.ptr + 4 >>> 2 >>> 0];
		}
		set_destructor(e) {
			O[this.ptr + 8 >>> 2 >>> 0] = e;
		}
		get_destructor() {
			return O[this.ptr + 8 >>> 2 >>> 0];
		}
		set_caught(e) {
			e = +!!e, b[this.ptr + 12 >>> 0] = e;
		}
		get_caught() {
			return b[this.ptr + 12 >>> 0] != 0;
		}
		set_rethrown(e) {
			e = +!!e, b[this.ptr + 13 >>> 0] = e;
		}
		get_rethrown() {
			return b[this.ptr + 13 >>> 0] != 0;
		}
		init(e, t) {
			this.set_adjusted_ptr(0), this.set_type(e), this.set_destructor(t);
		}
		set_adjusted_ptr(e) {
			O[this.ptr + 16 >>> 2 >>> 0] = e;
		}
		get_adjusted_ptr() {
			return O[this.ptr + 16 >>> 2 >>> 0];
		}
	}
	var ye = (e) => Si(e), be = (e) => {
		var t = D?.excPtr;
		if (!t) return ye(0), 0;
		var n = new ve(t);
		n.set_adjusted_ptr(t);
		var r = n.get_type();
		if (!r) return ye(0), t;
		for (var i of e) {
			if (!i || i === r) break;
			var a = n.ptr + 16;
			if (Di(i, r, a)) return ye(i), t;
		}
		return ye(r), t;
	};
	function k() {
		return be([]);
	}
	function xe(e) {
		return e >>>= 0, be([e]);
	}
	function Se(e, t, n) {
		return e >>>= 0, t >>>= 0, n >>>= 0, be([
			e,
			t,
			n
		]);
	}
	var Ce = () => {
		me.length || v("no exception to throw");
		var e = me.at(-1), t = e.excPtr;
		throw e.set_rethrown(!0), e.set_caught(!1), he++, Ei(t), D = new _(t), D;
	};
	function we(e, t, n) {
		throw e >>>= 0, t >>>= 0, n >>>= 0, new ve(e).init(t, n), Ei(e), D = new _(e), he++, D;
	}
	var Te = () => he;
	function Ee(e) {
		throw e >>>= 0, D ||= new _(e), D;
	}
	var A = {
		isAbs: (e) => e.charAt(0) === "/",
		splitPath: (e) => /^(\/?|)([\s\S]*?)((?:\.{1,2}|[^\/]+?|)(\.[^.\/]*|))(?:[\/]*)$/.exec(e).slice(1),
		normalizeArray: (e, t) => {
			for (var n = 0, r = e.length - 1; r >= 0; r--) {
				var i = e[r];
				i === "." ? e.splice(r, 1) : i === ".." ? (e.splice(r, 1), n++) : n && (e.splice(r, 1), n--);
			}
			if (t) for (; n; n--) e.unshift("..");
			return e;
		},
		normalize: (e) => {
			var t = A.isAbs(e), n = e.slice(-1) === "/";
			return e = A.normalizeArray(e.split("/").filter((e) => !!e), !t).join("/"), !e && !t && (e = "."), e && n && (e += "/"), (t ? "/" : "") + e;
		},
		dirname: (e) => {
			var t = A.splitPath(e), n = t[0], r = t[1];
			return !n && !r ? "." : (r &&= r.slice(0, -1), n + r);
		},
		basename: (e) => e && e.match(/([^\/]+|\/)\/*$/)[1],
		join: (...e) => A.normalize(e.join("/")),
		join2: (e, t) => A.normalize(e + "/" + t)
	}, j = () => (e) => (crypto.getRandomValues(e), 0), De = (e) => (De = j())(e), Oe = {
		resolve: (...e) => {
			for (var t = "", n = !1, r = e.length - 1; r >= -1 && !n; r--) {
				var i = r >= 0 ? e[r] : L.cwd();
				if (typeof i != "string") throw TypeError("Arguments to path.resolve must be strings");
				if (!i) return "";
				t = i + "/" + t, n = A.isAbs(i);
			}
			return t = A.normalizeArray(t.split("/").filter((e) => !!e), !n).join("/"), (n ? "/" : "") + t || ".";
		},
		relative: (e, t) => {
			e = Oe.resolve(e).slice(1), t = Oe.resolve(t).slice(1);
			function n(e) {
				for (var t = 0; t < e.length && e[t] === ""; t++);
				for (var n = e.length - 1; n >= 0 && e[n] === ""; n--);
				return t > n ? [] : e.slice(t, n - t + 1);
			}
			for (var r = n(e.split("/")), i = n(t.split("/")), a = Math.min(r.length, i.length), o = a, s = 0; s < a; s++) if (r[s] !== i[s]) {
				o = s;
				break;
			}
			for (var c = [], s = o; s < r.length; s++) c.push("..");
			return c = c.concat(i.slice(o)), c.join("/");
		}
	}, ke = globalThis.TextDecoder && new TextDecoder(), Ae = (e, t, n, r) => {
		var i = t + n;
		if (r) return i;
		for (; e[t] && !(t >= i);) ++t;
		return t;
	}, M = (e, t = 0, n, r) => {
		t >>>= 0;
		var i = Ae(e, t, n, r);
		if (i - t > 16 && e.buffer && ke) return ke.decode(e.subarray(t, i));
		for (var a = ""; t < i;) {
			var o = e[t++];
			if (!(o & 128)) {
				a += String.fromCharCode(o);
				continue;
			}
			var s = e[t++] & 63;
			if ((o & 224) == 192) {
				a += String.fromCharCode((o & 31) << 6 | s);
				continue;
			}
			var c = e[t++] & 63;
			if (o = (o & 240) == 224 ? (o & 15) << 12 | s << 6 | c : (o & 7) << 18 | s << 12 | c << 6 | e[t++] & 63, o < 65536) a += String.fromCharCode(o);
			else {
				var l = o - 65536;
				a += String.fromCharCode(55296 | l >> 10, 56320 | l & 1023);
			}
		}
		return a;
	}, je = [], Me = (e) => {
		for (var t = 0, n = 0; n < e.length; ++n) {
			var r = e.charCodeAt(n);
			r <= 127 ? t++ : r <= 2047 ? t += 2 : r >= 55296 && r <= 57343 ? (t += 4, ++n) : t += 3;
		}
		return t;
	}, Ne = (e, t, n, r) => {
		if (n >>>= 0, !(r > 0)) return 0;
		for (var i = n, a = n + r - 1, o = 0; o < e.length; ++o) {
			var s = e.codePointAt(o);
			if (s <= 127) {
				if (n >= a) break;
				t[n++ >>> 0] = s;
			} else if (s <= 2047) {
				if (n + 1 >= a) break;
				t[n++ >>> 0] = 192 | s >> 6, t[n++ >>> 0] = 128 | s & 63;
			} else if (s <= 65535) {
				if (n + 2 >= a) break;
				t[n++ >>> 0] = 224 | s >> 12, t[n++ >>> 0] = 128 | s >> 6 & 63, t[n++ >>> 0] = 128 | s & 63;
			} else {
				if (n + 3 >= a) break;
				t[n++ >>> 0] = 240 | s >> 18, t[n++ >>> 0] = 128 | s >> 12 & 63, t[n++ >>> 0] = 128 | s >> 6 & 63, t[n++ >>> 0] = 128 | s & 63, o++;
			}
		}
		return t[n >>> 0] = 0, n - i;
	}, Pe = (e, t, n) => {
		var r = n > 0 ? n : Me(e) + 1, i = Array(r), a = Ne(e, i, 0, i.length);
		return t && (i.length = a), i;
	}, Fe = () => {
		if (!je.length) {
			var e = null;
			if (globalThis.window?.prompt && (e = window.prompt("Input: "), e !== null && (e += "\n")), !e) return null;
			je = Pe(e, !0);
		}
		return je.shift();
	}, N = {
		ttys: [],
		init() {},
		shutdown() {},
		register(e, t) {
			N.ttys[e] = {
				input: [],
				output: [],
				ops: t
			}, L.registerDevice(e, N.stream_ops);
		},
		stream_ops: {
			open(e) {
				var t = N.ttys[e.node.rdev];
				if (!t) throw new L.ErrnoError(43);
				e.tty = t, e.seekable = !1;
			},
			close(e) {
				e.tty.ops.fsync(e.tty);
			},
			fsync(e) {
				e.tty.ops.fsync(e.tty);
			},
			read(e, t, n, r, i) {
				if (!e.tty || !e.tty.ops.get_char) throw new L.ErrnoError(60);
				for (var a = 0, o = 0; o < r; o++) {
					var s;
					try {
						s = e.tty.ops.get_char(e.tty);
					} catch {
						throw new L.ErrnoError(29);
					}
					if (s === void 0 && !a) throw new L.ErrnoError(6);
					if (s == null) break;
					a++, t[n + o] = s;
				}
				return a && (e.node.atime = Date.now()), a;
			},
			write(e, t, n, r, i) {
				if (!e.tty || !e.tty.ops.put_char) throw new L.ErrnoError(60);
				try {
					for (var a = 0; a < r; a++) e.tty.ops.put_char(e.tty, t[n + a]);
				} catch {
					throw new L.ErrnoError(29);
				}
				return r && (e.node.mtime = e.node.ctime = Date.now()), a;
			}
		},
		default_tty_ops: {
			get_char(e) {
				return Fe();
			},
			put_char(e, t) {
				t === null || t === 10 ? (d(M(e.output)), e.output = []) : t != 0 && e.output.push(t);
			},
			fsync(e) {
				e.output?.length > 0 && (d(M(e.output)), e.output = []);
			},
			ioctl_tcgets(e) {
				return {
					c_iflag: 25856,
					c_oflag: 5,
					c_cflag: 191,
					c_lflag: 35387,
					c_cc: [
						3,
						28,
						127,
						21,
						4,
						0,
						1,
						0,
						17,
						19,
						26,
						0,
						18,
						15,
						23,
						22,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0,
						0
					]
				};
			},
			ioctl_tcsets(e, t, n) {
				return 0;
			},
			ioctl_tiocgwinsz(e) {
				return [24, 80];
			}
		},
		default_tty1_ops: {
			put_char(e, t) {
				t === null || t === 10 ? (f(M(e.output)), e.output = []) : t != 0 && e.output.push(t);
			},
			fsync(e) {
				e.output?.length > 0 && (f(M(e.output)), e.output = []);
			}
		}
	}, P, Ie = (e, t) => P.fill(0, e, e + t), Le = (e, t) => Math.ceil(e / t) * t, Re = (e) => {
		e = Le(e, 65536);
		var t = xi(65536, e);
		return t && Ie(t, e), t;
	}, F = {
		ops_table: null,
		mount(e) {
			return F.createNode(null, "/", 16895, 0);
		},
		createNode(e, t, n, r) {
			if (L.isBlkdev(n) || L.isFIFO(n)) throw new L.ErrnoError(63);
			F.ops_table ||= {
				dir: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr,
						lookup: F.node_ops.lookup,
						mknod: F.node_ops.mknod,
						rename: F.node_ops.rename,
						unlink: F.node_ops.unlink,
						rmdir: F.node_ops.rmdir,
						readdir: F.node_ops.readdir,
						symlink: F.node_ops.symlink
					},
					stream: { llseek: F.stream_ops.llseek }
				},
				file: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr
					},
					stream: {
						llseek: F.stream_ops.llseek,
						read: F.stream_ops.read,
						write: F.stream_ops.write,
						mmap: F.stream_ops.mmap,
						msync: F.stream_ops.msync
					}
				},
				link: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr,
						readlink: F.node_ops.readlink
					},
					stream: {}
				},
				chrdev: {
					node: {
						getattr: F.node_ops.getattr,
						setattr: F.node_ops.setattr
					},
					stream: L.chrdev_stream_ops
				}
			};
			var i = L.createNode(e, t, n, r);
			return L.isDir(i.mode) ? (i.node_ops = F.ops_table.dir.node, i.stream_ops = F.ops_table.dir.stream, i.contents = {}) : L.isFile(i.mode) ? (i.node_ops = F.ops_table.file.node, i.stream_ops = F.ops_table.file.stream, i.usedBytes = 0, i.contents = F.emptyFileContents ??= /* @__PURE__ */ new Uint8Array()) : L.isLink(i.mode) ? (i.node_ops = F.ops_table.link.node, i.stream_ops = F.ops_table.link.stream) : L.isChrdev(i.mode) && (i.node_ops = F.ops_table.chrdev.node, i.stream_ops = F.ops_table.chrdev.stream), i.atime = i.mtime = i.ctime = Date.now(), e && (e.contents[t] = i, e.atime = e.mtime = e.ctime = i.atime), i;
		},
		getFileDataAsTypedArray(e) {
			return e.contents.subarray(0, e.usedBytes);
		},
		expandFileStorage(e, t) {
			var n = e.contents.length;
			if (!(n >= t)) {
				t = Math.max(t, n * (n < 1048576 ? 2 : 1.125) >>> 0), n && (t = Math.max(t, 256));
				var r = F.getFileDataAsTypedArray(e);
				e.contents = new Uint8Array(t), e.contents.set(r);
			}
		},
		resizeFileStorage(e, t) {
			if (e.usedBytes != t) {
				var n = e.contents;
				e.contents = new Uint8Array(t), e.contents.set(n.subarray(0, Math.min(t, e.usedBytes))), e.usedBytes = t;
			}
		},
		node_ops: {
			getattr(e) {
				var t = {};
				return t.dev = L.isChrdev(e.mode) ? e.id : 1, t.ino = e.id, t.mode = e.mode, t.nlink = 1, t.uid = 0, t.gid = 0, t.rdev = e.rdev, t.size = L.isDir(e.mode) ? 4096 : L.isFile(e.mode) ? e.usedBytes : L.isLink(e.mode) ? e.link.length : 0, t.atime = new Date(e.atime), t.mtime = new Date(e.mtime), t.ctime = new Date(e.ctime), t.blksize = 4096, t.blocks = Math.ceil(t.size / t.blksize), t;
			},
			setattr(e, t) {
				for (let n of [
					"mode",
					"atime",
					"mtime",
					"ctime"
				]) t[n] != null && (e[n] = t[n]);
				t.size !== void 0 && F.resizeFileStorage(e, t.size);
			},
			lookup(e, t) {
				throw F.doesNotExistError || (F.doesNotExistError = new L.ErrnoError(44), F.doesNotExistError.stack = "<generic error, no stack>"), F.doesNotExistError;
			},
			mknod(e, t, n, r) {
				return F.createNode(e, t, n, r);
			},
			rename(e, t, n) {
				var r;
				try {
					r = L.lookupNode(t, n);
				} catch {}
				if (r) {
					if (L.isDir(e.mode)) for (var i in r.contents) throw new L.ErrnoError(55);
					L.hashRemoveNode(r);
				}
				delete e.parent.contents[e.name], t.contents[n] = e, e.name = n, t.ctime = t.mtime = e.parent.ctime = e.parent.mtime = Date.now();
			},
			unlink(e, t) {
				delete e.contents[t], e.ctime = e.mtime = Date.now();
			},
			rmdir(e, t) {
				for (var n in L.lookupNode(e, t).contents) throw new L.ErrnoError(55);
				delete e.contents[t], e.ctime = e.mtime = Date.now();
			},
			readdir(e) {
				return [
					".",
					"..",
					...Object.keys(e.contents)
				];
			},
			symlink(e, t, n) {
				var r = F.createNode(e, t, 41471, 0);
				return r.link = n, r;
			},
			readlink(e) {
				if (!L.isLink(e.mode)) throw new L.ErrnoError(28);
				return e.link;
			}
		},
		stream_ops: {
			read(e, t, n, r, i) {
				var a = e.node.contents;
				if (i >= e.node.usedBytes) return 0;
				var o = Math.min(e.node.usedBytes - i, r);
				return t.set(a.subarray(i, i + o), n), o;
			},
			write(e, t, n, r, i, a) {
				if (t.buffer === b.buffer && (a = !1), !r) return 0;
				var o = e.node;
				return o.mtime = o.ctime = Date.now(), a ? (o.contents = t.subarray(n, n + r), o.usedBytes = r) : !o.usedBytes && !i ? (o.contents = t.slice(n, n + r), o.usedBytes = r) : (F.expandFileStorage(o, i + r), o.contents.set(t.subarray(n, n + r), i), o.usedBytes = Math.max(o.usedBytes, i + r)), r;
			},
			llseek(e, t, n) {
				var r = t;
				if (n === 1 ? r += e.position : n === 2 && L.isFile(e.node.mode) && (r += e.node.usedBytes), r < 0) throw new L.ErrnoError(28);
				return r;
			},
			mmap(e, t, n, r, i) {
				if (!L.isFile(e.node.mode)) throw new L.ErrnoError(43);
				var a, o, s = e.node.contents;
				if (!(i & 2) && s.buffer === b.buffer) o = !1, a = s.byteOffset;
				else {
					if (o = !0, a = Re(t), !a) throw new L.ErrnoError(48);
					s && ((n > 0 || n + t < s.length) && (s = s.subarray ? s.subarray(n, n + t) : Array.prototype.slice.call(s, n, n + t)), b.set(s, a >>> 0));
				}
				return {
					ptr: a,
					allocated: o
				};
			},
			msync(e, t, n, r, i) {
				return F.stream_ops.write(e, t, 0, r, n, !1), 0;
			}
		}
	}, I = (e) => {
		if (typeof e != "string") return e;
		var t = {
			r: 0,
			"r+": 2,
			w: 577,
			"w+": 578,
			a: 1089,
			"a+": 1090
		}[e];
		if (t === void 0) throw Error(`Unknown file open mode: ${e}`);
		return t;
	}, ze = (e) => (typeof e == "string" && (e = Pe(e, !0)), e.subarray || (e = new Uint8Array(e)), e), Be = (e, t) => {
		var n = 0;
		return e && (n |= 365), t && (n |= 146), n;
	}, Ve = async (e) => {
		var t = await l(e);
		return new Uint8Array(t);
	}, He = (...e) => L.createDataFile(...e), Ue = (e) => e, We = null, Ge = async () => We, Ke = 0, qe = null, Je = (e) => {
		Ke--, Ke || qe();
	}, Ye = (e) => {
		Ke || (We = new Promise((e) => qe = e)), Ke++;
	}, Xe = [], Ze = async (e, t) => {
		typeof Browser < "u" && Browser.init();
		for (var n of Xe) if (n.canHandle(t)) return n.handle(e, t);
		return e;
	}, Qe = async (e, t, n, r, i, a, o, s) => {
		var c = t ? Oe.resolve(A.join2(e, t)) : e, l = Ue(`cp ${c}`);
		Ye(l);
		try {
			var u = n;
			typeof n == "string" && (u = await Ve(n)), u = await Ze(u, c), s?.(), a || He(e, t, u, r, i, o);
		} finally {
			Je(l);
		}
	}, $e = (e, t, n, r, i, a, o, s, c, l) => {
		Qe(e, t, n, r, i, s, c, l).then(a).catch(o);
	}, L = {
		root: null,
		mounts: [],
		devices: {},
		streams: [],
		nextInode: 1,
		nameTable: null,
		currentPath: "/",
		initialized: !1,
		ignorePermissions: !0,
		filesystems: null,
		syncFSRequests: 0,
		ErrnoError: class {
			name = "ErrnoError";
			constructor(e) {
				this.errno = e;
			}
		},
		FSStream: class {
			shared = {};
			get object() {
				return this.node;
			}
			set object(e) {
				this.node = e;
			}
			get isRead() {
				return (this.flags & 2097155) != 1;
			}
			get isWrite() {
				return !!(this.flags & 2097155);
			}
			get isAppend() {
				return this.flags & 1024;
			}
			get flags() {
				return this.shared.flags;
			}
			set flags(e) {
				this.shared.flags = e;
			}
			get position() {
				return this.shared.position;
			}
			set position(e) {
				this.shared.position = e;
			}
		},
		FSNode: class {
			node_ops = {};
			stream_ops = {};
			readMode = 365;
			writeMode = 146;
			mounted = null;
			constructor(e, t, n, r) {
				e ||= this, this.parent = e, this.mount = e.mount, this.id = L.nextInode++, this.name = t, this.mode = n, this.rdev = r, this.atime = this.mtime = this.ctime = Date.now();
			}
			get read() {
				return (this.mode & this.readMode) === this.readMode;
			}
			set read(e) {
				e ? this.mode |= this.readMode : this.mode &= ~this.readMode;
			}
			get write() {
				return (this.mode & this.writeMode) === this.writeMode;
			}
			set write(e) {
				e ? this.mode |= this.writeMode : this.mode &= ~this.writeMode;
			}
			get isFolder() {
				return L.isDir(this.mode);
			}
			get isDevice() {
				return L.isChrdev(this.mode);
			}
			addListener(e, t = !1) {
				var n = {
					cb: e,
					exclusive: t
				}, r = this.listeners ??= /* @__PURE__ */ new Set();
				return r.add(n), {
					listeners: r,
					entry: n
				};
			}
			notifyListeners(e) {
				if (this.listeners) {
					var t;
					for (var n of this.listeners) n.exclusive ? (t ||= []).push(n) : n.cb(e);
					if (t) {
						var r = (this.exclTurn || 0) % t.length;
						this.exclTurn = r + 1, t[r].cb(e);
					}
				}
			}
		},
		lookupPath(e, t = {}) {
			if (!e) throw new L.ErrnoError(44);
			t.follow_mount ??= !0, A.isAbs(e) || (e = L.cwd() + "/" + e);
			linkloop: for (var n = 0; n < 40; n++) {
				for (var r = e.split("/").filter((e) => !!e), i = L.root, a = "/", o = 0; o < r.length; o++) {
					var s = o === r.length - 1;
					if (s && t.parent) break;
					if (r[o] !== ".") {
						if (r[o] === "..") {
							if (a = A.dirname(a), L.isRoot(i)) {
								e = a + "/" + r.slice(o + 1).join("/"), n--;
								continue linkloop;
							}
							i = i.parent;
							continue;
						}
						a = A.join2(a, r[o]);
						try {
							i = L.lookupNode(i, r[o]);
						} catch (e) {
							if (e?.errno === 44 && s && t.noent_okay) return { path: a };
							throw e;
						}
						if (L.isMountpoint(i) && (!s || t.follow_mount) && (i = i.mounted.root), L.isLink(i.mode) && (!s || t.follow)) {
							if (!i.node_ops.readlink) throw new L.ErrnoError(52);
							var c = i.node_ops.readlink(i);
							A.isAbs(c) || (c = A.dirname(a) + "/" + c), e = c + "/" + r.slice(o + 1).join("/");
							continue linkloop;
						}
					}
				}
				return {
					path: a,
					node: i
				};
			}
			throw new L.ErrnoError(32);
		},
		getPath(e) {
			for (var t;;) {
				if (L.isRoot(e)) {
					var n = e.mount.mountpoint;
					return t ? n[n.length - 1] === "/" ? n + t : `${n}/${t}` : n;
				}
				t = t ? `${e.name}/${t}` : e.name, e = e.parent;
			}
		},
		hashName(e, t) {
			for (var n = 0, r = 0; r < t.length; r++) n = (n << 5) - n + t.charCodeAt(r) | 0;
			return (e + n >>> 0) % L.nameTable.length;
		},
		hashAddNode(e) {
			var t = L.hashName(e.parent.id, e.name);
			e.name_next = L.nameTable[t], L.nameTable[t] = e;
		},
		hashRemoveNode(e) {
			var t = L.hashName(e.parent.id, e.name);
			if (L.nameTable[t] === e) L.nameTable[t] = e.name_next;
			else for (var n = L.nameTable[t]; n;) {
				if (n.name_next === e) {
					n.name_next = e.name_next;
					break;
				}
				n = n.name_next;
			}
		},
		lookupNode(e, t) {
			var n = L.mayLookup(e);
			if (n) throw new L.ErrnoError(n);
			for (var r = L.hashName(e.id, t), i = L.nameTable[r]; i; i = i.name_next) {
				var a = i.name;
				if (i.parent.id === e.id && a === t) return i;
			}
			return L.lookup(e, t);
		},
		createNode(e, t, n, r) {
			var i = new L.FSNode(e, t, n, r);
			return L.hashAddNode(i), i;
		},
		destroyNode(e) {
			L.hashRemoveNode(e);
		},
		isRoot(e) {
			return e === e.parent;
		},
		isMountpoint(e) {
			return !!e.mounted;
		},
		isFile(e) {
			return (e & 61440) == 32768;
		},
		isDir(e) {
			return (e & 61440) == 16384;
		},
		isLink(e) {
			return (e & 61440) == 40960;
		},
		isChrdev(e) {
			return (e & 61440) == 8192;
		},
		isBlkdev(e) {
			return (e & 61440) == 24576;
		},
		isFIFO(e) {
			return (e & 61440) == 4096;
		},
		isSocket(e) {
			return (e & 49152) == 49152;
		},
		flagsToPermissionString(e) {
			var t = [
				"r",
				"w",
				"rw"
			][e & 3];
			return e & 512 && (t += "w"), t;
		},
		nodePermissions(e, t) {
			return L.ignorePermissions ? 0 : t.includes("r") && !(e.mode & 292) || t.includes("w") && !(e.mode & 146) || t.includes("x") && !(e.mode & 73) ? 2 : 0;
		},
		mayLookup(e) {
			return L.isDir(e.mode) ? L.nodePermissions(e, "x") || (e.node_ops.lookup ? 0 : 2) : 54;
		},
		mayCreate(e, t) {
			if (!L.isDir(e.mode)) return 54;
			try {
				return L.lookupNode(e, t), 20;
			} catch {}
			return L.nodePermissions(e, "wx");
		},
		mayDelete(e, t, n) {
			var r;
			try {
				r = L.lookupNode(e, t);
			} catch (e) {
				return e.errno;
			}
			var i = L.nodePermissions(e, "wx");
			if (i) return i;
			if (n) {
				if (!L.isDir(r.mode)) return 54;
				if (L.isRoot(r) || L.getPath(r) === L.cwd()) return 10;
			} else if (L.isDir(r.mode)) return 31;
			return 0;
		},
		mayOpen(e, t) {
			if (!e) return 44;
			if (L.isLink(e.mode)) return 32;
			var n = L.flagsToPermissionString(t);
			return L.isDir(e.mode) && (n !== "r" || t & 576) ? 31 : L.nodePermissions(e, n);
		},
		checkOpExists(e, t) {
			if (!e) throw new L.ErrnoError(t);
			return e;
		},
		MAX_OPEN_FDS: 4096,
		nextfd() {
			for (var e = 0; e <= L.MAX_OPEN_FDS; e++) if (!L.streams[e]) return e;
			throw new L.ErrnoError(33);
		},
		getStreamChecked(e) {
			var t = L.getStream(e);
			if (!t) throw new L.ErrnoError(8);
			return t;
		},
		getStream: (e) => L.streams[e],
		createStream(e, t = -1) {
			return e = Object.assign(new L.FSStream(), e), t == -1 && (t = L.nextfd()), e.fd = t, L.streams[t] = e, e;
		},
		closeStream(e) {
			L.streams[e] = null;
		},
		dupStream(e, t = -1) {
			var n = L.createStream(e, t);
			return n.stream_ops?.dup?.(n), n;
		},
		doSetAttr(e, t, n) {
			var r = e?.stream_ops.setattr, i = r ? e : t;
			r ??= t.node_ops.setattr, L.checkOpExists(r, 63);
			try {
				r(i, n);
			} catch (e) {
				throw e instanceof RangeError ? new L.ErrnoError(22) : e;
			}
		},
		chrdev_stream_ops: {
			open(e) {
				e.stream_ops = L.getDevice(e.node.rdev).stream_ops, e.stream_ops.open?.(e);
			},
			llseek() {
				throw new L.ErrnoError(70);
			}
		},
		major: (e) => e >> 8,
		minor: (e) => e & 255,
		makedev: (e, t) => e << 8 | t,
		registerDevice(e, t) {
			L.devices[e] = { stream_ops: t };
		},
		getDevice: (e) => L.devices[e],
		getMounts(e) {
			for (var t = [], n = [e]; n.length;) {
				var r = n.pop();
				t.push(r), n.push(...r.mounts);
			}
			return t;
		},
		syncfs(e, t) {
			typeof e == "function" && (t = e, e = !1), L.syncFSRequests++, L.syncFSRequests > 1 && f(`warning: ${L.syncFSRequests} FS.syncfs operations in flight at once, probably just doing extra work`);
			var n = L.getMounts(L.root.mount), r = 0;
			function i(e) {
				return L.syncFSRequests--, t(e);
			}
			function a(e) {
				if (e) return a.errored ? void 0 : (a.errored = !0, i(e));
				++r >= n.length && i(null);
			}
			for (var o of n) o.type.syncfs ? o.type.syncfs(o, e, a) : a(null);
		},
		mount(e, t, n) {
			var r = n === "/", i = !n, a;
			if (r && L.root) throw new L.ErrnoError(10);
			if (!r && !i) {
				var o = L.lookupPath(n, { follow_mount: !1 });
				if (n = o.path, a = o.node, L.isMountpoint(a)) throw new L.ErrnoError(10);
				if (!L.isDir(a.mode)) throw new L.ErrnoError(54);
			}
			var s = {
				type: e,
				opts: t,
				mountpoint: n,
				mounts: []
			}, c = e.mount(s);
			return c.mount = s, s.root = c, r ? L.root = c : a && (a.mounted = s, a.mount && a.mount.mounts.push(s)), c;
		},
		unmount(e) {
			var t = L.lookupPath(e, { follow_mount: !1 });
			if (!L.isMountpoint(t.node)) throw new L.ErrnoError(28);
			var n = t.node, r = n.mounted, i = L.getMounts(r);
			for (var [a, o] of Object.entries(L.nameTable)) for (; o;) {
				var s = o.name_next;
				i.includes(o.mount) && L.destroyNode(o), o = s;
			}
			n.mounted = null;
			var c = n.mount.mounts.indexOf(r);
			n.mount.mounts.splice(c, 1);
		},
		lookup(e, t) {
			return e.node_ops.lookup(e, t);
		},
		mknod(e, t, n) {
			var r = L.lookupPath(e, { parent: !0 }).node, i = A.basename(e);
			if (!i) throw new L.ErrnoError(28);
			if (i === "." || i === "..") throw new L.ErrnoError(20);
			var a = L.mayCreate(r, i);
			if (a) throw new L.ErrnoError(a);
			if (!r.node_ops.mknod) throw new L.ErrnoError(63);
			return r.node_ops.mknod(r, i, t, n);
		},
		statfs(e) {
			return L.statfsNode(L.lookupPath(e, { follow: !0 }).node);
		},
		statfsStream(e) {
			return L.statfsNode(e.node);
		},
		statfsNode(e) {
			var t = {
				bsize: 4096,
				frsize: 4096,
				blocks: 1e6,
				bfree: 5e5,
				bavail: 5e5,
				files: L.nextInode,
				ffree: L.nextInode - 1,
				fsid: 42,
				flags: 2,
				namelen: 255
			};
			return e.node_ops.statfs && Object.assign(t, e.node_ops.statfs(e.mount.opts.root)), t;
		},
		create(e, t = 438) {
			return t &= 4095, t |= 32768, L.mknod(e, t, 0);
		},
		mkdir(e, t = 511) {
			return t &= 1023, t |= 16384, L.mknod(e, t, 0);
		},
		mkdirTree(e, t) {
			var n = e.split("/"), r = "";
			for (var i of n) if (i) {
				(r || A.isAbs(e)) && (r += "/"), r += i;
				try {
					L.mkdir(r, t);
				} catch (e) {
					if (e.errno != 20) throw e;
				}
			}
		},
		mkdev(e, t, n) {
			return n === void 0 && (n = t, t = 438), t |= 8192, L.mknod(e, t, n);
		},
		symlink(e, t) {
			if (!Oe.resolve(e)) throw new L.ErrnoError(44);
			var n = L.lookupPath(t, { parent: !0 }).node;
			if (!n) throw new L.ErrnoError(44);
			var r = A.basename(t), i = L.mayCreate(n, r);
			if (i) throw new L.ErrnoError(i);
			if (!n.node_ops.symlink) throw new L.ErrnoError(63);
			return n.node_ops.symlink(n, r, e);
		},
		link(e, t, n) {
			var r = L.lookupPath(t, { parent: !0 }).node;
			if (!r) throw new L.ErrnoError(44);
			var i = A.basename(t), a = L.mayCreate(r, i);
			if (a) throw new L.ErrnoError(a);
			if (!r.node_ops.link) throw new L.ErrnoError(34);
			return r.node_ops.link(r, i, e, n);
		},
		rename(e, t) {
			var n = A.dirname(e), r = A.dirname(t), i = A.basename(e), a = A.basename(t), o = L.lookupPath(e, { parent: !0 }), s = o.node, c;
			if (o = L.lookupPath(t, { parent: !0 }), c = o.node, !s || !c) throw new L.ErrnoError(44);
			if (s.mount !== c.mount) throw new L.ErrnoError(75);
			var l = L.lookupNode(s, i), u = Oe.relative(e, r);
			if (u.charAt(0) !== ".") throw new L.ErrnoError(28);
			if (u = Oe.relative(t, n), u.charAt(0) !== ".") throw new L.ErrnoError(55);
			var d;
			try {
				d = L.lookupNode(c, a);
			} catch {}
			if (l !== d) {
				var f = L.isDir(l.mode), p = L.mayDelete(s, i, f);
				if (p || (p = d ? L.mayDelete(c, a, f) : L.mayCreate(c, a), p)) throw new L.ErrnoError(p);
				if (!s.node_ops.rename) throw new L.ErrnoError(63);
				if (L.isMountpoint(l) || d && L.isMountpoint(d)) throw new L.ErrnoError(10);
				if (c !== s && (p = L.nodePermissions(s, "w"), p)) throw new L.ErrnoError(p);
				L.hashRemoveNode(l);
				try {
					s.node_ops.rename(l, c, a), l.parent = c;
				} catch (e) {
					throw e;
				} finally {
					L.hashAddNode(l);
				}
			}
		},
		rmdir(e) {
			var t = L.lookupPath(e, { parent: !0 }).node, n = A.basename(e), r = L.lookupNode(t, n), i = L.mayDelete(t, n, !0);
			if (i) throw new L.ErrnoError(i);
			if (!t.node_ops.rmdir) throw new L.ErrnoError(63);
			if (L.isMountpoint(r)) throw new L.ErrnoError(10);
			t.node_ops.rmdir(t, n), L.destroyNode(r);
		},
		readdir(e) {
			var t = L.lookupPath(e, { follow: !0 }).node;
			return L.checkOpExists(t.node_ops.readdir, 54)(t);
		},
		unlink(e) {
			var t = L.lookupPath(e, { parent: !0 }).node;
			if (!t) throw new L.ErrnoError(44);
			var n = A.basename(e), r = L.lookupNode(t, n), i = L.mayDelete(t, n, !1);
			if (i) throw new L.ErrnoError(i);
			if (!t.node_ops.unlink) throw new L.ErrnoError(63);
			if (L.isMountpoint(r)) throw new L.ErrnoError(10);
			t.node_ops.unlink(t, n), L.destroyNode(r);
		},
		readlink(e) {
			var t = L.lookupPath(e).node;
			if (!t) throw new L.ErrnoError(44);
			if (!t.node_ops.readlink) throw new L.ErrnoError(28);
			return t.node_ops.readlink(t);
		},
		stat(e, t) {
			var n = L.lookupPath(e, { follow: !t }).node;
			return L.checkOpExists(n.node_ops.getattr, 63)(n);
		},
		fstat(e) {
			var t = L.getStreamChecked(e), n = t.node, r = t.stream_ops.getattr, i = r ? t : n;
			return r ??= n.node_ops.getattr, L.checkOpExists(r, 63), r(i);
		},
		lstat(e) {
			return L.stat(e, !0);
		},
		doChmod(e, t, n, r) {
			L.doSetAttr(e, t, {
				mode: n & 4095 | t.mode & -4096,
				ctime: Date.now(),
				dontFollow: r
			});
		},
		chmod(e, t, n) {
			var r = typeof e == "string" ? L.lookupPath(e, { follow: !n }).node : e;
			L.doChmod(null, r, t, n);
		},
		lchmod(e, t) {
			L.chmod(e, t, !0);
		},
		fchmod(e, t) {
			var n = L.getStreamChecked(e);
			L.doChmod(n, n.node, t, !1);
		},
		doChown(e, t, n) {
			L.doSetAttr(e, t, {
				timestamp: Date.now(),
				dontFollow: n
			});
		},
		chown(e, t, n, r) {
			var i = typeof e == "string" ? L.lookupPath(e, { follow: !r }).node : e;
			L.doChown(null, i, r);
		},
		lchown(e, t, n) {
			L.chown(e, t, n, !0);
		},
		fchown(e, t, n) {
			var r = L.getStreamChecked(e);
			L.doChown(r, r.node, !1);
		},
		doTruncate(e, t, n) {
			if (L.isDir(t.mode)) throw new L.ErrnoError(31);
			if (!L.isFile(t.mode)) throw new L.ErrnoError(28);
			var r = L.nodePermissions(t, "w");
			if (r) throw new L.ErrnoError(r);
			L.doSetAttr(e, t, {
				size: n,
				timestamp: Date.now()
			});
		},
		truncate(e, t) {
			if (t < 0) throw new L.ErrnoError(28);
			var n = typeof e == "string" ? L.lookupPath(e, { follow: !0 }).node : e;
			L.doTruncate(null, n, t);
		},
		ftruncate(e, t) {
			var n = L.getStreamChecked(e);
			if (t < 0 || !(n.flags & 2097155)) throw new L.ErrnoError(28);
			L.doTruncate(n, n.node, t);
		},
		utime(e, t, n, r) {
			var i = L.lookupPath(e, { follow: !r });
			L.doSetAttr(null, i.node, {
				atime: t,
				mtime: n,
				dontFollow: r
			});
		},
		open(e, t, n = 438) {
			if (e === "") throw new L.ErrnoError(44);
			t = I(t), n = t & 64 ? n & 4095 | 32768 : 0;
			var r, i;
			if (typeof e == "object") r = e;
			else {
				i = e.endsWith("/");
				var a = L.lookupPath(e, {
					follow: !(t & 131072),
					noent_okay: !0
				});
				r = a.node, e = a.path;
			}
			var o = !1;
			if (t & 64) {
				if (r) {
					if (t & 128) throw new L.ErrnoError(20);
				} else if (i) throw new L.ErrnoError(31);
				else r = L.mknod(e, n | 511, 0), o = !0;
			}
			if (!r) throw new L.ErrnoError(44);
			if (L.isChrdev(r.mode) && (t &= -513), t & 65536 && !L.isDir(r.mode)) throw new L.ErrnoError(54);
			if (!o) {
				var s = L.mayOpen(r, t);
				if (s) throw new L.ErrnoError(s);
			}
			t & 512 && !o && L.truncate(r, 0), t &= -131713;
			var c = L.createStream({
				node: r,
				path: L.getPath(r),
				flags: t,
				seekable: !0,
				position: 0,
				stream_ops: r.stream_ops,
				ungotten: [],
				error: !1
			});
			return c.stream_ops.open && c.stream_ops.open(c), o && L.chmod(r, n & 511), c;
		},
		close(e) {
			if (L.isClosed(e)) throw new L.ErrnoError(8);
			e.getdents &&= null, e.node?.notifyListeners(32);
			try {
				e.stream_ops.close && e.stream_ops.close(e);
			} catch (e) {
				throw e;
			} finally {
				L.closeStream(e.fd);
			}
			e.fd = null;
		},
		isClosed(e) {
			return e.fd === null;
		},
		llseek(e, t, n) {
			if (L.isClosed(e)) throw new L.ErrnoError(8);
			if (!e.seekable || !e.stream_ops.llseek) throw new L.ErrnoError(70);
			if (n != 0 && n != 1 && n != 2) throw new L.ErrnoError(28);
			return e.position = e.stream_ops.llseek(e, t, n), e.ungotten = [], e.position;
		},
		read(e, t, n, r, i) {
			if (r < 0 || i < 0) throw new L.ErrnoError(28);
			if (L.isClosed(e) || (e.flags & 2097155) == 1) throw new L.ErrnoError(8);
			if (L.isDir(e.node.mode)) throw new L.ErrnoError(31);
			if (!e.stream_ops.read) throw new L.ErrnoError(28);
			var a = i !== void 0;
			if (!a) i = e.position;
			else if (!e.seekable) throw new L.ErrnoError(70);
			var o = e.stream_ops.read(e, t, n, r, i);
			return a || (e.position += o), o;
		},
		write(e, t, n, r, i, a) {
			if (r < 0 || i < 0) throw new L.ErrnoError(28);
			if (L.isClosed(e) || !(e.flags & 2097155)) throw new L.ErrnoError(8);
			if (L.isDir(e.node.mode)) throw new L.ErrnoError(31);
			if (!e.stream_ops.write) throw new L.ErrnoError(28);
			e.seekable && e.flags & 1024 && L.llseek(e, 0, 2);
			var o = i !== void 0;
			if (!o) i = e.position;
			else if (!e.seekable) throw new L.ErrnoError(70);
			var s = e.stream_ops.write(e, t, n, r, i, a);
			return o || (e.position += s), s;
		},
		mmap(e, t, n, r, i) {
			if (r & 2 && !(i & 2) && (e.flags & 2097155) != 2 || (e.flags & 2097155) == 1) throw new L.ErrnoError(2);
			if (!e.stream_ops.mmap) throw new L.ErrnoError(43);
			if (!t) throw new L.ErrnoError(28);
			return e.stream_ops.mmap(e, t, n, r, i);
		},
		msync(e, t, n, r, i) {
			return e.stream_ops.msync ? e.stream_ops.msync(e, t, n, r, i) : 0;
		},
		ioctl(e, t, n) {
			if (!e.stream_ops.ioctl) throw new L.ErrnoError(59);
			return e.stream_ops.ioctl(e, t, n);
		},
		readFile(e, t = {}) {
			t.flags = t.flags ?? 0, t.encoding = t.encoding ?? "binary", t.encoding !== "utf8" && t.encoding !== "binary" && v(`Invalid encoding type "${t.encoding}"`);
			var n = L.open(e, t.flags), r = L.stat(e).size, i = new Uint8Array(r);
			return L.read(n, i, 0, r, 0), t.encoding === "utf8" && (i = M(i)), L.close(n), i;
		},
		writeFile(e, t, n = {}) {
			n.flags = n.flags ?? 577;
			var r = L.open(e, n.flags, n.mode);
			t = ze(t), L.write(r, t, 0, t.byteLength, void 0, n.canOwn), L.close(r);
		},
		cwd: () => L.currentPath,
		chdir(e) {
			var t = L.lookupPath(e, { follow: !0 });
			if (t.node === null) throw new L.ErrnoError(44);
			if (!L.isDir(t.node.mode)) throw new L.ErrnoError(54);
			var n = L.nodePermissions(t.node, "x");
			if (n) throw new L.ErrnoError(n);
			L.currentPath = t.path;
		},
		createDefaultDirectories() {
			L.mkdir("/tmp"), L.mkdir("/home"), L.mkdir("/home/web_user");
		},
		createDefaultDevices() {
			L.mkdir("/dev"), L.registerDevice(L.makedev(1, 3), {
				read: () => 0,
				write: (e, t, n, r, i) => r,
				llseek: () => 0
			}), L.mkdev("/dev/null", L.makedev(1, 3)), N.register(L.makedev(5, 0), N.default_tty_ops), N.register(L.makedev(6, 0), N.default_tty1_ops), L.mkdev("/dev/tty", L.makedev(5, 0)), L.mkdev("/dev/tty1", L.makedev(6, 0));
			var e = /* @__PURE__ */ new Uint8Array(1024), t = 0, n = () => (t ||= (De(e), e.byteLength), e[--t]);
			L.createDevice("/dev", "random", n), L.createDevice("/dev", "urandom", n), L.mkdir("/dev/shm"), L.mkdir("/dev/shm/tmp");
		},
		createSpecialDirectories() {
			L.mkdir("/proc");
			var e = L.mkdir("/proc/self");
			L.mkdir("/proc/self/fd"), L.mount({ mount() {
				var t = L.createNode(e, "fd", 16895, 73);
				return t.stream_ops = { llseek: F.stream_ops.llseek }, t.node_ops = {
					lookup(e, t) {
						var n = +t, r = L.getStreamChecked(n), i = {
							parent: null,
							mount: { mountpoint: "fake" },
							node_ops: { readlink: () => r.path },
							id: n + 1
						};
						return i.parent = i, i;
					},
					readdir() {
						return Array.from(L.streams.entries()).filter(([e, t]) => t).map(([e, t]) => e.toString());
					}
				}, t;
			} }, {}, "/proc/self/fd");
		},
		createStandardStreams(e, t, n) {
			e ? L.createDevice("/dev", "stdin", e) : L.symlink("/dev/tty", "/dev/stdin"), t ? L.createDevice("/dev", "stdout", null, t) : L.symlink("/dev/tty", "/dev/stdout"), n ? L.createDevice("/dev", "stderr", null, n) : L.symlink("/dev/tty1", "/dev/stderr"), L.open("/dev/stdin", 0), L.open("/dev/stdout", 1), L.open("/dev/stderr", 1);
		},
		staticInit() {
			L.nameTable = Array(4096), L.mount(F, {}, "/"), L.createDefaultDirectories(), L.createDefaultDevices(), L.createSpecialDirectories(), L.filesystems = { MEMFS: F };
		},
		init(e, t, n) {
			L.initialized = !0, L.createStandardStreams(e, t, n);
		},
		quit() {
			L.initialized = !1;
			for (var e of L.streams) e && L.close(e);
		},
		findObject(e, t) {
			var n = L.analyzePath(e, t);
			return n.exists ? n.object : null;
		},
		analyzePath(e, t) {
			try {
				var n = L.lookupPath(e, { follow: !t });
				e = n.path;
			} catch {}
			var r = {
				isRoot: !1,
				exists: !1,
				error: 0,
				name: null,
				path: null,
				object: null,
				parentExists: !1,
				parentPath: null,
				parentObject: null
			};
			try {
				var n = L.lookupPath(e, { parent: !0 });
				r.parentExists = !0, r.parentPath = n.path, r.parentObject = n.node, r.name = A.basename(e), n = L.lookupPath(e, { follow: !t }), r.exists = !0, r.path = n.path, r.object = n.node, r.name = n.node.name, r.isRoot = n.path === "/";
			} catch (e) {
				r.error = e.errno;
			}
			return r;
		},
		createPath(e, t, n, r) {
			e = typeof e == "string" ? e : L.getPath(e);
			for (var i = t.split("/").reverse(); i.length;) {
				var a = i.pop();
				if (a) {
					var o = A.join2(e, a);
					try {
						L.mkdir(o);
					} catch (e) {
						if (e.errno != 20) throw e;
					}
					e = o;
				}
			}
			return o;
		},
		createFile(e, t, n, r, i) {
			var a = A.join2(typeof e == "string" ? e : L.getPath(e), t), o = Be(r, i);
			return L.create(a, o);
		},
		createDataFile(e, t, n, r, i, a) {
			var o = t;
			e && (e = typeof e == "string" ? e : L.getPath(e), o = t ? A.join2(e, t) : e);
			var s = Be(r, i), c = L.create(o, s);
			if (n) {
				n = ze(n), L.chmod(c, s | 146);
				var l = L.open(c, 577);
				L.write(l, n, 0, n.length, 0, a), L.close(l), L.chmod(c, s);
			}
		},
		createDevice(e, t, n, r) {
			var i = A.join2(typeof e == "string" ? e : L.getPath(e), t), a = Be(!!n, !!r);
			L.createDevice.major ??= 64;
			var o = L.makedev(L.createDevice.major++, 0);
			return L.registerDevice(o, {
				open(e) {
					e.seekable = !1;
				},
				close(e) {
					r?.buffer?.length && r(10);
				},
				read(e, t, r, i, a) {
					for (var o = 0, s = 0; s < i; s++) {
						var c;
						try {
							c = n();
						} catch {
							throw new L.ErrnoError(29);
						}
						if (c === void 0 && !o) throw new L.ErrnoError(6);
						if (c == null) break;
						o++, t[r + s] = c;
					}
					return o && (e.node.atime = Date.now()), o;
				},
				write(e, t, n, i, a) {
					for (var o = 0; o < i; o++) try {
						r(t[n + o]);
					} catch {
						throw new L.ErrnoError(29);
					}
					return i && (e.node.mtime = e.node.ctime = Date.now()), o;
				}
			}), L.mkdev(i, a, o);
		},
		forceLoadFile(e) {
			if (e.isDevice || e.isFolder || e.link || e.contents) return !0;
			if (globalThis.XMLHttpRequest) v("Lazy loading should have been performed (contents set) in createLazyFile, but it was not. Lazy loading only works in web workers. Use --embed-file or --preload-file in emcc on the main thread.");
			else try {
				e.contents = u(e.url);
			} catch {
				throw new L.ErrnoError(29);
			}
		},
		createLazyFile(e, t, n, i, a) {
			class o {
				lengthKnown = !1;
				chunks = [];
				get(e) {
					if (!(e > this.length - 1 || e < 0)) {
						var t = e % this.chunkSize, n = e / this.chunkSize | 0;
						return this.getter(n)[t];
					}
				}
				setDataGetter(e) {
					this.getter = e;
				}
				cacheLength() {
					var e = new XMLHttpRequest();
					e.open("HEAD", n, !1), e.send(null), e.status >= 200 && e.status < 300 || e.status === 304 || v(`Couldn't load ${n}. Status: ${e.status}`);
					var t = Number(e.getResponseHeader("Content-length")), r, i = (r = e.getResponseHeader("Accept-Ranges")) && r === "bytes", a = (r = e.getResponseHeader("Content-Encoding")) && r === "gzip", o = 1048576;
					i || (o = t);
					var s = (e, r) => {
						e > r && v(`invalid range (${e}, ${r}) or no bytes requested!`), r > t - 1 && v(`only ${t} bytes available! programmer error!`);
						var i = new XMLHttpRequest();
						return i.open("GET", n, !1), t !== o && i.setRequestHeader("Range", `bytes=${e}-${r}`), i.responseType = "arraybuffer", i.overrideMimeType && i.overrideMimeType("text/plain; charset=x-user-defined"), i.send(null), i.status >= 200 && i.status < 300 || i.status === 304 || v(`Couldn't load ${n}. Status: ${i.status}`), i.response === void 0 ? Pe(i.responseText ?? "", !0) : new Uint8Array(i.response || []);
					}, c = this;
					c.setDataGetter((e) => {
						var n = e * o, r = (e + 1) * o - 1;
						return r = Math.min(r, t - 1), c.chunks[e] === void 0 && (c.chunks[e] = s(n, r)), c.chunks[e] === void 0 && v("doXHR failed!"), c.chunks[e];
					}), (a || !t) && (o = t = 1, t = this.getter(0).length, o = t, d("LazyFiles on gzip forces download of the whole file when length is accessed")), this._length = t, this._chunkSize = o, this.lengthKnown = !0;
				}
				get length() {
					return this.lengthKnown || this.cacheLength(), this._length;
				}
				get chunkSize() {
					return this.lengthKnown || this.cacheLength(), this._chunkSize;
				}
			}
			if (globalThis.XMLHttpRequest) {
				r || v("Cannot do synchronous binary XHRs outside webworkers in modern browsers. Use --embed-file or --preload-file in emcc");
				var s = {
					isDevice: !1,
					contents: new o()
				};
			} else var s = {
				isDevice: !1,
				url: n
			};
			var c = L.createFile(e, t, s, i, a);
			s.contents ? c.contents = s.contents : s.url && (c.contents = null, c.url = s.url), Object.defineProperties(c, { usedBytes: { get: function() {
				return this.contents.length;
			} } });
			var l = {};
			for (let [e, t] of Object.entries(c.stream_ops)) l[e] = (...e) => (L.forceLoadFile(c), t(...e));
			function u(e, t, n, r, i) {
				var a = e.node.contents;
				if (i >= a.length) return 0;
				var o = Math.min(a.length - i, r);
				if (a.slice) for (var s = 0; s < o; s++) t[n + s] = a[i + s];
				else for (var s = 0; s < o; s++) t[n + s] = a.get(i + s);
				return o;
			}
			return l.read = (e, t, n, r, i) => (L.forceLoadFile(c), u(e, t, n, r, i)), l.mmap = (e, t, n, r, i) => {
				L.forceLoadFile(c);
				var a = Re(t);
				if (!a) throw new L.ErrnoError(48);
				return u(e, b, a, t, n), {
					ptr: a,
					allocated: !0
				};
			}, c.stream_ops = l, c;
		}
	}, et = (e, t, n) => (e >>>= 0, e ? M(P, e, t, n) : ""), R, z, B = {
		currentUmask: 18,
		calculateAt(e, t, n) {
			if (A.isAbs(t)) return t;
			var r = e === -100 ? L.cwd() : B.getStreamFromFD(e).path;
			if (t.length == 0) {
				if (!n) throw new L.ErrnoError(44);
				return r;
			}
			return r + "/" + t;
		},
		writeStat(e, t) {
			O[e >>> 2 >>> 0] = t.dev, O[e + 4 >>> 2 >>> 0] = t.mode, O[e + 8 >>> 2 >>> 0] = t.nlink, O[e + 12 >>> 2 >>> 0] = t.uid, O[e + 16 >>> 2 >>> 0] = t.gid, O[e + 20 >>> 2 >>> 0] = t.rdev, z[e + 24 >>> 3 >>> 0] = BigInt(t.size), R[e + 32 >>> 2 >>> 0] = 4096, R[e + 36 >>> 2 >>> 0] = t.blocks;
			var n = t.atime.getTime(), r = t.mtime.getTime(), i = t.ctime.getTime();
			return z[e + 40 >>> 3 >>> 0] = BigInt(Math.floor(n / 1e3)), O[e + 48 >>> 2 >>> 0] = n % 1e3 * 1e3 * 1e3, z[e + 56 >>> 3 >>> 0] = BigInt(Math.floor(r / 1e3)), O[e + 64 >>> 2 >>> 0] = r % 1e3 * 1e3 * 1e3, z[e + 72 >>> 3 >>> 0] = BigInt(Math.floor(i / 1e3)), O[e + 80 >>> 2 >>> 0] = i % 1e3 * 1e3 * 1e3, z[e + 88 >>> 3 >>> 0] = BigInt(t.ino), 0;
		},
		writeStatFs(e, t) {
			O[e + 4 >>> 2 >>> 0] = t.bsize, O[e + 60 >>> 2 >>> 0] = t.bsize, z[e + 8 >>> 3 >>> 0] = BigInt(t.blocks), z[e + 16 >>> 3 >>> 0] = BigInt(t.bfree), z[e + 24 >>> 3 >>> 0] = BigInt(t.bavail), z[e + 32 >>> 3 >>> 0] = BigInt(t.files), z[e + 40 >>> 3 >>> 0] = BigInt(t.ffree), O[e + 48 >>> 2 >>> 0] = t.fsid, O[e + 64 >>> 2 >>> 0] = t.flags, O[e + 56 >>> 2 >>> 0] = t.namelen;
		},
		doMsync(e, t, n, r, i) {
			if (!L.isFile(t.node.mode)) throw new L.ErrnoError(43);
			if (r & 2) return 0;
			var a = P.subarray(e >>> 0, e + n >>> 0);
			L.msync(t, a, i, n, r);
		},
		getStreamFromFD(e) {
			return L.getStreamChecked(e);
		},
		varargs: void 0,
		getStr(e) {
			return et(e);
		}
	};
	function V(e, t) {
		e >>>= 0;
		try {
			return e = B.getStr(e), L.chmod(e, t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function tt(e) {
		try {
			var t = B.getStreamFromFD(e);
			return L.dupStream(t).fd;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function nt(e, t, n, r) {
		t >>>= 0;
		try {
			if (t = B.getStr(t), t = B.calculateAt(e, t), n & -8) return -28;
			var i = L.lookupPath(t, { follow: !0 }).node;
			if (!i) return -44;
			var a = "";
			return n & 4 && (a += "r"), n & 2 && (a += "w"), n & 1 && (a += "x"), a && L.nodePermissions(i, a) ? -2 : 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function rt(e, t, n, r) {
		n = w(n), r = w(r);
		try {
			if (isNaN(n) || isNaN(r)) return -22;
			if (t != 0) return -138;
			if (n < 0 || r < 0) return -28;
			if (!B.getStreamFromFD(e).seekable) return -70;
			var i = L.fstat(e).size, a = n + r;
			return a > i && L.ftruncate(e, a), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function H(e, t) {
		try {
			return L.fchmod(e, t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var it = () => {
		var e = R[B.varargs >>> 2 >>> 0];
		return B.varargs += 4, e;
	}, at = it, U;
	function ot(e, t, n) {
		n >>>= 0, B.varargs = n;
		try {
			var r = B.getStreamFromFD(e);
			switch (t) {
				case 0:
					var i = it();
					if (i < 0) return -28;
					for (; L.streams[i];) i++;
					return L.dupStream(r, i).fd;
				case 1:
				case 2: return 0;
				case 3: return r.flags;
				case 4:
					var i = it(), a = 289792;
					return r.flags = r.flags & ~a | i & a, 0;
				case 12:
					var i = at(), o = 0;
					return U[i + o >>> 1 >>> 0] = 2, 0;
				case 13:
				case 14: return 0;
			}
			return -28;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function st(e, t) {
		t >>>= 0;
		try {
			return B.writeStat(t, L.fstat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var ct = (e, t, n) => Ne(e, P, t, n);
	function lt(e, t) {
		e >>>= 0, t >>>= 0;
		try {
			if (!t) return -28;
			var n = L.cwd(), r = Me(n) + 1;
			return t < r ? -68 : (ct(n, e, t), r);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ut(e, t, n) {
		t >>>= 0, n >>>= 0;
		try {
			var r = B.getStreamFromFD(e);
			r.getdents ||= L.readdir(r.path);
			for (var i = 280, a = 0, o = L.llseek(r, 0, 1), s = Math.floor(o / i), c = Math.min(r.getdents.length, s + Math.floor(n / i)), l = s; l < c; l++) {
				var u, d, f = r.getdents[l];
				if (f === ".") u = r.node.id, d = 4;
				else if (f === "..") u = L.lookupPath(r.path, { parent: !0 }).node.id, d = 4;
				else {
					var p;
					try {
						p = L.lookupNode(r.node, f);
					} catch (e) {
						if (e?.errno === 28) continue;
						throw e;
					}
					u = p.id, d = L.isChrdev(p.mode) ? 2 : L.isDir(p.mode) ? 4 : L.isLink(p.mode) ? 10 : 8;
				}
				z[t + a >>> 3 >>> 0] = BigInt(u), z[t + a + 8 >>> 3 >>> 0] = BigInt((l + 1) * i), U[t + a + 16 >>> 1 >>> 0] = 280, b[t + a + 18 >>> 0] = d, ct(f, t + a + 19, 256), a += i;
			}
			return L.llseek(r, l * i, 0), a;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function dt(e, t, n) {
		n >>>= 0, B.varargs = n;
		try {
			var r = B.getStreamFromFD(e);
			switch (t) {
				case 21509: return r.tty ? 0 : -59;
				case 21505:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tcgets) {
						var i = r.tty.ops.ioctl_tcgets(r), a = at();
						R[a >>> 2 >>> 0] = i.c_iflag || 0, R[a + 4 >>> 2 >>> 0] = i.c_oflag || 0, R[a + 8 >>> 2 >>> 0] = i.c_cflag || 0, R[a + 12 >>> 2 >>> 0] = i.c_lflag || 0;
						for (var o = 0; o < 32; o++) b[a + o + 17 >>> 0] = i.c_cc[o] || 0;
						return 0;
					}
					return 0;
				case 21510:
				case 21511:
				case 21512: return r.tty ? 0 : -59;
				case 21506:
				case 21507:
				case 21508:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tcsets) {
						for (var a = at(), s = R[a >>> 2 >>> 0], c = R[a + 4 >>> 2 >>> 0], l = R[a + 8 >>> 2 >>> 0], u = R[a + 12 >>> 2 >>> 0], d = [], o = 0; o < 32; o++) d.push(b[a + o + 17 >>> 0]);
						return r.tty.ops.ioctl_tcsets(r.tty, t, {
							c_iflag: s,
							c_oflag: c,
							c_cflag: l,
							c_lflag: u,
							c_cc: d
						});
					}
					return 0;
				case 21519:
					if (!r.tty) return -59;
					var a = at();
					return R[a >>> 2 >>> 0] = 0, 0;
				case 21520: return r.tty ? -28 : -59;
				case 21537:
				case 21531:
					var a = at();
					return L.ioctl(r, t, a);
				case 21523:
					if (!r.tty) return -59;
					if (r.tty.ops.ioctl_tiocgwinsz) {
						var f = r.tty.ops.ioctl_tiocgwinsz(r.tty), a = at();
						U[a >>> 1 >>> 0] = f[0], U[a + 2 >>> 1 >>> 0] = f[1];
					}
					return 0;
				case 21524: return r.tty ? 0 : -59;
				case 21515: return r.tty ? 0 : -59;
				default: return -28;
			}
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ft(e, t) {
		e >>>= 0, t >>>= 0;
		try {
			return e = B.getStr(e), B.writeStat(t, L.lstat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function pt(e, t, n, r) {
		t >>>= 0, n >>>= 0;
		try {
			t = B.getStr(t);
			var i = r & 256, a = r & 4096;
			return r &= -6401, t = B.calculateAt(e, t, a), B.writeStat(n, i ? L.lstat(t) : L.stat(t));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function mt(e, t, n, r) {
		t >>>= 0, r >>>= 0, B.varargs = r;
		try {
			t = B.getStr(t), t = B.calculateAt(e, t);
			var i = r ? it() : 0;
			return n & 64 && (i &= ~B.currentUmask), L.open(t, n, i).fd;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function ht(e, t, n, r) {
		t >>>= 0, n >>>= 0, r >>>= 0;
		try {
			if (t = B.getStr(t), t = B.calculateAt(e, t), r <= 0) return -28;
			var i = L.readlink(t), a = Math.min(r, Me(i)), o = b[n + a >>> 0];
			return ct(i, n, r + 1), b[n + a >>> 0] = o, a;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function gt(e, t, n, r) {
		t >>>= 0, r >>>= 0;
		try {
			return t = B.getStr(t), r = B.getStr(r), t = B.calculateAt(e, t), r = B.calculateAt(n, r), L.rename(t, r), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function _t(e) {
		e >>>= 0;
		try {
			return e = B.getStr(e), L.rmdir(e), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function vt(e, t) {
		e >>>= 0, t >>>= 0;
		try {
			return e = B.getStr(e), B.writeStat(t, L.stat(e));
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function yt(e, t, n) {
		e >>>= 0, n >>>= 0;
		try {
			return e = B.getStr(e), n = B.getStr(n), n = B.calculateAt(t, n), L.symlink(e, n), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function bt(e, t, n) {
		t >>>= 0;
		try {
			if (t = B.getStr(t), t = B.calculateAt(e, t), !n) L.unlink(t);
			else if (n === 512) L.rmdir(t);
			else return -28;
			return 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var xt = () => v(""), St = {}, Ct = (e) => {
		for (; e.length;) {
			var t = e.pop();
			e.pop()(t);
		}
	};
	function wt(e) {
		return this.fromWireType(O[e >>> 2 >>> 0]);
	}
	var Tt = {}, Et = {}, Dt = {};
	class Ot extends Error {
		constructor(e) {
			super(e), this.name = "InternalError";
		}
	}
	var kt = (e) => {
		throw new Ot(e);
	}, At = (e, t, n) => {
		e.forEach((e) => Dt[e] = t);
		function r(t) {
			var r = n(t);
			r.length !== e.length && kt("Mismatched type converter count");
			for (var i = 0; i < e.length; ++i) Mt(e[i], r[i]);
		}
		var i = Array(t.length), a = [], o = 0;
		for (let [e, n] of t.entries()) Et.hasOwnProperty(n) ? i[e] = Et[n] : (a.push(n), Tt.hasOwnProperty(n) || (Tt[n] = []), Tt[n].push(() => {
			i[e] = Et[n], ++o, o === a.length && r(i);
		}));
		a.length === 0 && r(i);
	}, jt = function(e) {
		e >>>= 0;
		var t = St[e];
		delete St[e];
		var n = t.rawConstructor, r = t.rawDestructor, i = t.fields, a = i.map((e) => e.getterReturnType).concat(i.map((e) => e.setterArgumentType));
		At([e], a, (e) => {
			var a = {};
			for (var [o, s] of i.entries()) {
				let t = e[o], n = s.getter, r = s.getterContext, c = e[o + i.length], l = s.setter, u = s.setterContext;
				a[s.fieldName] = {
					read: (e) => t.fromWireType(n(r, e)),
					write: (e, t) => {
						var n = [];
						l(u, e, c.toWireType(n, t)), Ct(n);
					},
					optional: t.optional
				};
			}
			return [{
				name: t.name,
				fromWireType: (e) => {
					var t = {};
					for (var n in a) t[n] = a[n].read(e);
					return r(e), t;
				},
				toWireType: (e, t) => {
					for (var i in a) if (!(i in t) && !a[i].optional) throw TypeError(`Missing field: "${i}"`);
					var o = n();
					for (i in a) a[i].write(o, t[i]);
					return e !== null && e.push(r, o), o;
				},
				readValueFromPointer: wt,
				destructorFunction: r
			}];
		});
	}, W = (e) => {
		e >>>= 0;
		for (var t = "";;) {
			var n = P[e++ >>> 0];
			if (!n) return t;
			t += String.fromCharCode(n);
		}
	};
	class G extends Error {
		constructor(e) {
			super(e), this.name = "BindingError";
		}
	}
	var K = (e) => {
		throw new G(e);
	};
	function q(e, t, n = {}) {
		var r = t.name;
		if (e || K(`type "${r}" must have a positive integer typeid pointer`), Et.hasOwnProperty(e)) {
			if (n.ignoreDuplicateRegistrations) return;
			K(`Cannot register type '${r}' twice`);
		}
		if (Et[e] = t, delete Dt[e], Tt.hasOwnProperty(e)) {
			var i = Tt[e];
			delete Tt[e], i.forEach((e) => e());
		}
	}
	function Mt(e, t, n = {}) {
		return q(e, t, n);
	}
	var J, Nt, Pt = (e, t, n) => {
		switch (t) {
			case 1: return n ? (e) => b[e >>> 0] : (e) => P[e >>> 0];
			case 2: return n ? (e) => U[e >>> 1 >>> 0] : (e) => J[e >>> 1 >>> 0];
			case 4: return n ? (e) => R[e >>> 2 >>> 0] : (e) => O[e >>> 2 >>> 0];
			case 8: return n ? (e) => z[e >>> 3 >>> 0] : (e) => Nt[e >>> 3 >>> 0];
			default: throw TypeError(`invalid integer width (${t}): ${e}`);
		}
	}, Ft = function(e, t, n, r, i) {
		e >>>= 0, t >>>= 0, n >>>= 0, t = W(t);
		let a = r === 0n, o = (e) => e;
		if (a) {
			let e = n * 8;
			o = (t) => BigInt.asUintN(e, t), i = o(i);
		}
		Mt(e, {
			name: t,
			fromWireType: o,
			toWireType: (e, t) => (typeof t == "number" && (t = BigInt(t)), t),
			readValueFromPointer: Pt(t, n, !a),
			destructorFunction: null
		});
	};
	function It(e, t, n, r) {
		e >>>= 0, t >>>= 0, t = W(t), Mt(e, {
			name: t,
			fromWireType: function(e) {
				return !!e;
			},
			toWireType: function(e, t) {
				return t ? n : r;
			},
			readValueFromPointer: function(e) {
				return this.fromWireType(P[e >>> 0]);
			},
			destructorFunction: null
		});
	}
	var Lt = (e) => ({
		count: e.count,
		deleteScheduled: e.deleteScheduled,
		preservePointerOnDelete: e.preservePointerOnDelete,
		ptr: e.ptr,
		ptrType: e.ptrType,
		smartPtr: e.smartPtr,
		smartPtrType: e.smartPtrType
	}), Rt = (e) => {
		function t(e) {
			return e.$$.ptrType.registeredClass.name;
		}
		K(t(e) + " instance already deleted");
	}, zt = !1, Bt = (e) => {}, Vt = (e) => {
		e.smartPtr ? e.smartPtrType.rawDestructor(e.smartPtr) : e.ptrType.registeredClass.rawDestructor(e.ptr);
	}, Ht = (e) => {
		--e.count.value, e.count.value === 0 && Vt(e);
	}, Ut = (e) => globalThis.FinalizationRegistry ? (zt = new FinalizationRegistry((e) => {
		Ht(e.$$);
	}), Ut = (e) => {
		var t = e.$$;
		if (t.smartPtr) {
			var n = { $$: t };
			zt.register(e, n, e);
		}
		return e;
	}, Bt = (e) => zt.unregister(e), Ut(e)) : (Ut = (e) => e, e), Wt = [], Gt = () => {
		for (; Wt.length;) {
			var e = Wt.pop();
			e.$$.deleteScheduled = !1, e.delete();
		}
	}, Kt, qt = () => {
		let e = Jt.prototype;
		Object.assign(e, {
			isAliasOf(e) {
				if (!(this instanceof Jt) || !(e instanceof Jt)) return !1;
				var t = this.$$.ptrType.registeredClass, n = this.$$.ptr;
				e.$$ = e.$$;
				for (var r = e.$$.ptrType.registeredClass, i = e.$$.ptr; t.baseClass;) n = t.upcast(n), t = t.baseClass;
				for (; r.baseClass;) i = r.upcast(i), r = r.baseClass;
				return t === r && n === i;
			},
			clone() {
				if (this.$$.ptr || Rt(this), this.$$.preservePointerOnDelete) return this.$$.count.value += 1, this;
				var e = Ut(Object.create(Object.getPrototypeOf(this), { $$: { value: Lt(this.$$) } }));
				return e.$$.count.value += 1, e.$$.deleteScheduled = !1, e;
			},
			delete() {
				this.$$.ptr || Rt(this), this.$$.deleteScheduled && !this.$$.preservePointerOnDelete && K("Object already scheduled for deletion"), Bt(this), Ht(this.$$), this.$$.preservePointerOnDelete || (this.$$.smartPtr = void 0, this.$$.ptr = void 0);
			},
			isDeleted() {
				return !this.$$.ptr;
			},
			deleteLater() {
				return this.$$.ptr || Rt(this), this.$$.deleteScheduled && !this.$$.preservePointerOnDelete && K("Object already scheduled for deletion"), Wt.push(this), Wt.length === 1 && Kt && Kt(Gt), this.$$.deleteScheduled = !0, this;
			}
		});
		let t = Symbol.dispose;
		t && (e[t] = e.delete);
	};
	function Jt() {}
	var Yt = (e, t) => Object.defineProperty(t, "name", { value: e }), Xt = {}, Zt = (e, t, n) => {
		if (e[t].overloadTable === void 0) {
			var r = e[t];
			e[t] = function(...r) {
				return e[t].overloadTable.hasOwnProperty(r.length) || K(`Function '${n}' called with an invalid number of arguments (${r.length}) - expects one of (${e[t].overloadTable})!`), e[t].overloadTable[r.length].apply(this, r);
			}, e[t].overloadTable = [], e[t].overloadTable[r.argCount] = r;
		}
	}, Qt = (e, n, r) => {
		t.hasOwnProperty(e) ? ((r === void 0 || t[e].overloadTable !== void 0 && t[e].overloadTable[r] !== void 0) && K(`Cannot register public name '${e}' twice`), Zt(t, e, e), t[e].overloadTable.hasOwnProperty(r) && K(`Cannot register multiple overloads of a function with the same number of arguments (${r})!`), t[e].overloadTable[r] = n) : (t[e] = n, t[e].argCount = r);
	}, $t = 48, en = 57, tn = (e) => {
		e = e.replace(/[^a-zA-Z0-9_]/g, "$");
		var t = e.charCodeAt(0);
		return t >= $t && t <= en ? `_${e}` : e;
	};
	function nn(e, t, n, r, i, a, o, s) {
		this.name = e, this.constructor = t, this.instancePrototype = n, this.rawDestructor = r, this.baseClass = i, this.getActualType = a, this.upcast = o, this.downcast = s, this.pureVirtualFunctions = [];
	}
	var rn = (e, t, n) => {
		for (; t !== n;) t.upcast || K(`Expected null or instance of ${n.name}, got an instance of ${t.name}`), e = t.upcast(e), t = t.baseClass;
		return e;
	}, an = (e) => {
		if (e === null) return "null";
		var t = typeof e;
		return t === "object" || t === "array" || t === "function" ? e.toString() : "" + e;
	};
	function on(e, t) {
		if (t === null) return this.isReference && K(`null is not a valid ${this.name}`), 0;
		t.$$ || K(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || K(`Cannot pass deleted object as a pointer of type ${this.name}`);
		var n = t.$$.ptrType.registeredClass;
		return rn(t.$$.ptr, n, this.registeredClass);
	}
	function sn(e, t) {
		var n;
		if (t === null) return this.isReference && K(`null is not a valid ${this.name}`), this.isSmartPointer ? (n = this.rawConstructor(), e !== null && e.push(this.rawDestructor, n), n) : 0;
		(!t || !t.$$) && K(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || K(`Cannot pass deleted object as a pointer of type ${this.name}`), !this.isConst && t.$$.ptrType.isConst && K(`Cannot convert argument of type ${t.$$.smartPtrType ? t.$$.smartPtrType.name : t.$$.ptrType.name} to parameter type ${this.name}`);
		var r = t.$$.ptrType.registeredClass;
		if (n = rn(t.$$.ptr, r, this.registeredClass), this.isSmartPointer) switch (t.$$.smartPtr === void 0 && K("Passing raw pointer to smart pointer is illegal"), this.sharingPolicy) {
			case 0:
				t.$$.smartPtrType === this ? n = t.$$.smartPtr : K(`Cannot convert argument of type ${t.$$.smartPtrType ? t.$$.smartPtrType.name : t.$$.ptrType.name} to parameter type ${this.name}`);
				break;
			case 1:
				n = t.$$.smartPtr;
				break;
			case 2:
				if (t.$$.smartPtrType === this) n = t.$$.smartPtr;
				else {
					var i = t.clone();
					n = this.rawShare(n, Z.toHandle(() => i.delete())), e !== null && e.push(this.rawDestructor, n);
				}
				break;
			default: K("Unsupported sharing policy");
		}
		return n;
	}
	function cn(e, t) {
		if (t === null) return this.isReference && K(`null is not a valid ${this.name}`), 0;
		t.$$ || K(`Cannot pass "${an(t)}" as a ${this.name}`), t.$$.ptr || K(`Cannot pass deleted object as a pointer of type ${this.name}`), t.$$.ptrType.isConst && K(`Cannot convert argument of type ${t.$$.ptrType.name} to parameter type ${this.name}`);
		var n = t.$$.ptrType.registeredClass;
		return rn(t.$$.ptr, n, this.registeredClass);
	}
	var ln = (e, t, n) => {
		if (t === n) return e;
		if (n.baseClass === void 0) return null;
		var r = ln(e, t, n.baseClass);
		return r === null ? null : n.downcast(r);
	}, un = {}, dn = (e, t) => {
		for (t === void 0 && K("ptr should not be undefined"); e.baseClass;) t = e.upcast(t), e = e.baseClass;
		return t;
	}, fn = (e, t) => (t = dn(e, t), un[t]), pn = (e, t) => ((!t.ptrType || !t.ptr) && kt("makeClassHandle requires ptr and ptrType"), !!t.smartPtrType != !!t.smartPtr && kt("Both smartPtrType and smartPtr must be specified"), t.count = { value: 1 }, Ut(Object.create(e, { $$: {
		value: t,
		writable: !0
	} })));
	function mn(e) {
		var t = this.getPointee(e);
		if (!t) return this.destructor(e), null;
		var n = fn(this.registeredClass, t);
		if (n !== void 0) {
			if (n.$$.count.value === 0) return n.$$.ptr = t, n.$$.smartPtr = e, n.clone();
			var r = n.clone();
			return this.destructor(e), r;
		}
		function i() {
			return this.isSmartPointer ? pn(this.registeredClass.instancePrototype, {
				ptrType: this.pointeeType,
				ptr: t,
				smartPtrType: this,
				smartPtr: e
			}) : pn(this.registeredClass.instancePrototype, {
				ptrType: this,
				ptr: e
			});
		}
		var a = Xt[this.registeredClass.getActualType(t)];
		if (!a) return i.call(this);
		var o = this.isConst ? a.constPointerType : a.pointerType, s = ln(t, this.registeredClass, o.registeredClass);
		return s === null ? i.call(this) : this.isSmartPointer ? pn(o.registeredClass.instancePrototype, {
			ptrType: o,
			ptr: s,
			smartPtrType: this,
			smartPtr: e
		}) : pn(o.registeredClass.instancePrototype, {
			ptrType: o,
			ptr: s
		});
	}
	var hn = () => {
		Object.assign(gn.prototype, {
			getPointee(e) {
				return this.rawGetPointee && (e = this.rawGetPointee(e)), e;
			},
			destructor(e) {
				this.rawDestructor?.(e);
			},
			readValueFromPointer: wt,
			fromWireType: mn
		});
	};
	function gn(e, t, n, r, i, a, o, s, c, l, u) {
		this.name = e, this.registeredClass = t, this.isReference = n, this.isConst = r, this.isSmartPointer = i, this.pointeeType = a, this.sharingPolicy = o, this.rawGetPointee = s, this.rawConstructor = c, this.rawShare = l, this.rawDestructor = u, !i && t.baseClass === void 0 ? r ? (this.toWireType = on, this.destructorFunction = null) : (this.toWireType = cn, this.destructorFunction = null) : this.toWireType = sn;
	}
	var _n = (e, n, r) => {
		t.hasOwnProperty(e) || kt("Replacing nonexistent public symbol"), t[e].overloadTable !== void 0 && r !== void 0 ? t[e].overloadTable[r] = n : (t[e] = n, t[e].argCount = r);
	}, vn = (e, t, n = [], r = !1) => {
		var i = T(t)(...n);
		function a(t) {
			return e[0] == "p" ? t >>> 0 : t;
		}
		return a(i);
	}, yn = (e, t, n = !1) => (...r) => vn(e, t, r, n), Y = (e, t, n = !1) => {
		e = W(e);
		function r() {
			return e.includes("p") ? yn(e, t, n) : T(t);
		}
		var i = r();
		return typeof i != "function" && K(`unknown function pointer with signature ${e}: ${t}`), i;
	};
	class bn extends Error {}
	var xn = (e) => {
		var t = yi(e), n = W(t);
		return Q(t), n;
	}, Sn = (e, t) => {
		var n = [], r = {};
		function i(e) {
			if (!r[e] && !Et[e]) {
				if (Dt[e]) {
					Dt[e].forEach(i);
					return;
				}
				n.push(e), r[e] = !0;
			}
		}
		throw t.forEach(i), new bn(`${e}: ` + n.map(xn).join([", "]));
	};
	function Cn(e, t, n, r, i, a, o, s, c, l, u, d, f) {
		e >>>= 0, t >>>= 0, n >>>= 0, r >>>= 0, i >>>= 0, a >>>= 0, o >>>= 0, s >>>= 0, c >>>= 0, l >>>= 0, u >>>= 0, d >>>= 0, f >>>= 0, u = W(u), a = Y(i, a), s &&= Y(o, s), l &&= Y(c, l), f = Y(d, f);
		var p = tn(u);
		Qt(p, function() {
			Sn(`Cannot construct ${u} due to unbound types`, [r]);
		}), At([
			e,
			t,
			n
		], r ? [r] : [], (t) => {
			t = t[0];
			var n, i;
			r ? (n = t.registeredClass, i = n.instancePrototype) : i = Jt.prototype;
			var o = Yt(u, function(...e) {
				if (Object.getPrototypeOf(this) !== c) throw new G(`Use 'new' to construct ${u}`);
				if (d.constructor_body === void 0) throw new G(`${u} has no accessible constructor`);
				var t = d.constructor_body[e.length];
				if (t === void 0) throw new G(`Tried to invoke ctor of ${u} with invalid number of parameters (${e.length}) - expected (${Object.keys(d.constructor_body).toString()}) parameters instead!`);
				return t.apply(this, e);
			}), c = Object.create(i, { constructor: { value: o } });
			o.prototype = c;
			var d = new nn(u, o, c, f, n, a, s, l);
			d.baseClass && (d.baseClass.__derivedClasses ??= [], d.baseClass.__derivedClasses.push(d));
			var m = new gn(u, d, !0, !1, !1), h = new gn(u + "*", d, !1, !1, !1), g = new gn(u + " const*", d, !1, !0, !1);
			return Xt[e] = {
				pointerType: h,
				constPointerType: g
			}, _n(p, o), [
				m,
				h,
				g
			];
		});
	}
	var wn = [], X = [
		0,
		1,
		,
		1,
		null,
		1,
		!0,
		1,
		!1,
		1
	], Tn = [];
	function En(e) {
		if (e >>>= 0, e > 9 && --X[e + 1] === 0) {
			var t = X[e];
			X[e] = void 0;
			var n = Tn[e];
			n && (Tn[e] = void 0, n(t)), wn.push(e);
		}
	}
	var Z = {
		toValue: (e) => (e || K(`Cannot use deleted val. handle = ${e}`), X[e]),
		toHandle: (e) => {
			switch (e) {
				case void 0: return 2;
				case null: return 4;
				case !0: return 6;
				case !1: return 8;
				default: {
					let t = wn.pop() || X.length;
					return X[t] = e, X[t + 1] = 1, t;
				}
			}
		}
	}, Dn = {
		name: "emscripten::val",
		fromWireType: (e) => {
			var t = Z.toValue(e);
			return En(e), t;
		},
		toWireType: (e, t) => Z.toHandle(t),
		readValueFromPointer: wt,
		destructorFunction: null
	};
	function On(e) {
		return e >>>= 0, Mt(e, Dn);
	}
	var kn = (e, t, n) => {
		switch (t) {
			case 1: return n ? function(e) {
				return this.fromWireType(b[e >>> 0]);
			} : function(e) {
				return this.fromWireType(P[e >>> 0]);
			};
			case 2: return n ? function(e) {
				return this.fromWireType(U[e >>> 1 >>> 0]);
			} : function(e) {
				return this.fromWireType(J[e >>> 1 >>> 0]);
			};
			case 4: return n ? function(e) {
				return this.fromWireType(R[e >>> 2 >>> 0]);
			} : function(e) {
				return this.fromWireType(O[e >>> 2 >>> 0]);
			};
			default: throw TypeError(`invalid integer width (${t}): ${e}`);
		}
	};
	function An(e) {
		return e ? e === 1 ? "number" : "string" : "object";
	}
	function jn(e, n, r, i, a) {
		e >>>= 0, n >>>= 0, r >>>= 0, n = W(n);
		let o = An(a);
		switch (o) {
			case "object": {
				function t() {}
				t.values = {}, Mt(e, {
					name: n,
					constructor: t,
					valueType: o,
					fromWireType: function(e) {
						return this.constructor.values[e];
					},
					toWireType: (e, t) => t.value,
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, t);
				break;
			}
			case "number":
				var s = {};
				Mt(e, {
					name: n,
					keysMap: s,
					valueType: o,
					fromWireType: (e) => e,
					toWireType: (e, t) => t,
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, s), delete t[n].argCount;
				break;
			case "string":
				var c = {}, l = {}, s = {};
				Mt(e, {
					name: n,
					valuesMap: c,
					reverseMap: l,
					keysMap: s,
					valueType: o,
					fromWireType: function(e) {
						return this.reverseMap[e];
					},
					toWireType: function(e, t) {
						return this.valuesMap[t];
					},
					readValueFromPointer: kn(n, r, i),
					destructorFunction: null
				}), Qt(n, s), delete t[n].argCount;
		}
	}
	var Mn = (e, t) => {
		var n = Et[e];
		return n === void 0 && K(`${t} has unknown type ${xn(e)}`), n;
	};
	function Nn(e, t, n) {
		e >>>= 0, t >>>= 0;
		var r = Mn(e, "enum");
		switch (t = W(t), r.valueType) {
			case "object":
				var i = r.constructor, a = Object.create(r.constructor.prototype, {
					value: { value: n },
					constructor: { value: Yt(`${r.name}_${t}`, function() {}) }
				});
				i.values[n] = a, i[t] = a;
				break;
			case "number":
				r.keysMap[t] = n;
				break;
			case "string": r.valuesMap[t] = n, r.reverseMap[n] = t, r.keysMap[t] = t;
		}
	}
	var Pn, Fn, In = (e, t) => {
		switch (t) {
			case 4: return function(e) {
				return this.fromWireType(Pn[e >>> 2 >>> 0]);
			};
			case 8: return function(e) {
				return this.fromWireType(Fn[e >>> 3 >>> 0]);
			};
			default: throw TypeError(`invalid float width (${t}): ${e}`);
		}
	}, Ln = function(e, t, n) {
		e >>>= 0, t >>>= 0, n >>>= 0, t = W(t), Mt(e, {
			name: t,
			fromWireType: (e) => e,
			toWireType: (e, t) => t,
			readValueFromPointer: In(t, n),
			destructorFunction: null
		});
	};
	function Rn(e) {
		for (var t = 1; t < e.length; ++t) if (e[t] !== null && e[t].destructorFunction === void 0) return !0;
		return !1;
	}
	var zn = {
		ftf: function(e, t, n, r, i, a, o) {
			return function() {
				return a(n(r));
			};
		},
		ftft: function(e, t, n, r, i, a, o, s, c) {
			return function(e) {
				var t = s(null, e), i = n(r, t);
				return c(t), a(i);
			};
		},
		ftfn: function(e, t, n, r, i, a, o, s) {
			return function(e) {
				return a(n(r, s(null, e)));
			};
		},
		ftfnn: function(e, t, n, r, i, a, o, s, c) {
			return function(e, t) {
				return a(n(r, s(null, e), c(null, t)));
			};
		},
		fffn: function(e, t, n, r, i, a, o, s) {
			return function(e) {
				n(r, s(null, e));
			};
		},
		ftfnnn: function(e, t, n, r, i, a, o, s, c, l) {
			return function(e, t, i) {
				return a(n(r, s(null, e), c(null, t), l(null, i)));
			};
		},
		ftfnt: function(e, t, n, r, i, a, o, s, c, l) {
			return function(e, t) {
				var i = s(null, e), o = c(null, t), u = n(r, i, o);
				return l(o), a(u);
			};
		}
	};
	function Bn(e, t, n, r) {
		let i = [
			t ? "t" : "f",
			n ? "t" : "f",
			r ? "t" : "f"
		];
		for (let n = t ? 1 : 2; n < e.length; ++n) {
			let t = e[n], r = "";
			r = t.destructorFunction === void 0 ? "u" : t.destructorFunction === null ? "n" : "t", i.push(r);
		}
		return i.join("");
	}
	function Vn(e, t, n, r, i, a) {
		var o = t.length;
		o < 2 && K("argTypes array size mismatch! Must at least get return value and receiver (this) types!");
		for (var s = t[1] !== null && n !== null, c = Rn(t), l = !t[0].isVoid, u = t[0], d = t[1], f = [
			e,
			K,
			r,
			i,
			Ct,
			u.fromWireType.bind(u),
			d?.toWireType.bind(d)
		], p = 2; p < o; ++p) {
			var m = t[p];
			f.push(m.toWireType.bind(m));
		}
		if (!c) for (var p = s ? 1 : 2; p < t.length; ++p) t[p].destructorFunction !== null && f.push(t[p].destructorFunction);
		return Yt(e, zn[Bn(t, s, l, a)](...f));
	}
	var Hn = (e, t) => {
		for (var n = [], r = 0; r < e; r++) n.push(O[t + r * 4 >>> 2 >>> 0]);
		return n;
	}, Un = (e) => {
		e = e.trim();
		let t = e.indexOf("(");
		return t === -1 ? e : e.slice(0, t);
	};
	function Wn(e, t, n, r, i, a, o, s) {
		e >>>= 0, n >>>= 0, r >>>= 0, i >>>= 0, a >>>= 0;
		var c = Hn(t, n);
		e = W(e), e = Un(e), i = Y(r, i, o), Qt(e, function() {
			Sn(`Cannot call ${e} due to unbound types`, c);
		}, t - 1), At([], c, (n) => {
			var r = [n[0], null].concat(n.slice(1));
			return _n(e, Vn(e, r, null, i, a, o), t - 1), [];
		});
	}
	var Gn = function(e, t, n, r, i) {
		e >>>= 0, t >>>= 0, n >>>= 0, t = W(t);
		let a = r === 0, o = (e) => e;
		if (a) {
			var s = 32 - 8 * n;
			o = (e) => e << s >>> s, i = o(i);
		}
		Mt(e, {
			name: t,
			fromWireType: o,
			toWireType: (e, t) => t,
			readValueFromPointer: Pt(t, n, r !== 0),
			destructorFunction: null
		});
	};
	function Kn(e, t, n) {
		e >>>= 0, n >>>= 0;
		var r = [
			Int8Array,
			Uint8Array,
			Int16Array,
			Uint16Array,
			Int32Array,
			Uint32Array,
			Float32Array,
			Float64Array,
			BigInt64Array,
			BigUint64Array
		][t];
		function i(e) {
			var t = O[e >>> 2 >>> 0], n = O[e + 4 >>> 2 >>> 0];
			return new r(b.buffer, n, t);
		}
		n = W(n), Mt(e, {
			name: n,
			fromWireType: i,
			readValueFromPointer: i
		}, { ignoreDuplicateRegistrations: !0 });
	}
	function qn(e, t) {
		e >>>= 0, t >>>= 0, t = W(t);
		var n = !0;
		Mt(e, {
			name: t,
			fromWireType(e) {
				var t = O[e >>> 2 >>> 0], r = e + 4, i;
				if (n) i = et(r, t, !0);
				else {
					i = "";
					for (var a = 0; a < t; ++a) i += String.fromCharCode(P[r + a >>> 0]);
				}
				return Q(e), i;
			},
			toWireType(e, t) {
				t instanceof ArrayBuffer && (t = new Uint8Array(t));
				var r, i = typeof t == "string";
				i || ArrayBuffer.isView(t) && t.BYTES_PER_ELEMENT == 1 || K("Cannot pass non-string to std::string"), r = n && i ? Me(t) : t.length;
				var a = bi(4 + r + 1), o = a + 4;
				if (O[a >>> 2 >>> 0] = r, i) {
					if (n) ct(t, o, r + 1);
					else for (var s = 0; s < r; ++s) {
						var c = t.charCodeAt(s);
						c > 255 && (Q(a), K("String has UTF-16 code units that do not fit in 8 bits")), P[o + s >>> 0] = c;
					}
				} else P.set(t, o >>> 0);
				return e !== null && e.push(Q, a), a;
			},
			readValueFromPointer: wt,
			destructorFunction(e) {
				Q(e);
			}
		});
	}
	var Jn = globalThis.TextDecoder ? new TextDecoder("utf-16le") : void 0, Yn = (e, t, n) => {
		var r = e >>> 1, i = Ae(J, r, t / 2, n);
		if (i - r > 16 && Jn) return Jn.decode(J.subarray(r >>> 0, i >>> 0));
		for (var a = "", o = r; o < i; ++o) {
			var s = J[o >>> 0];
			a += String.fromCharCode(s);
		}
		return a;
	}, Xn = (e, t, n = 2147483647) => {
		if (n < 2) return 0;
		n -= 2;
		for (var r = t, i = n < e.length * 2 ? n / 2 : e.length, a = 0; a < i; ++a) {
			var o = e.charCodeAt(a);
			U[t >>> 1 >>> 0] = o, t += 2;
		}
		return U[t >>> 1 >>> 0] = 0, t - r;
	}, Zn = (e) => e.length * 2, Qn = (e, t, n) => {
		for (var r = "", i = e >>> 2, a = 0; !(a >= t / 4); a++) {
			var o = O[i + a >>> 0];
			if (!o && !n) break;
			r += String.fromCodePoint(o);
		}
		return r;
	}, $n = (e, t, n = 2147483647) => {
		if (t >>>= 0, n < 4) return 0;
		for (var r = t, i = r + n - 4, a = 0; a < e.length; ++a) {
			var o = e.codePointAt(a);
			if (o > 65535 && a++, R[t >>> 2 >>> 0] = o, t += 4, t + 4 > i) break;
		}
		return R[t >>> 2 >>> 0] = 0, t - r;
	}, er = (e) => {
		for (var t = 0, n = 0; n < e.length; ++n) e.codePointAt(n) > 65535 && n++, t += 4;
		return t;
	};
	function tr(e, t, n) {
		e >>>= 0, t >>>= 0, n >>>= 0, n = W(n);
		var r, i, a;
		t === 2 ? (r = Yn, i = Xn, a = Zn) : (r = Qn, i = $n, a = er), Mt(e, {
			name: n,
			fromWireType: (e) => {
				var n = O[e >>> 2 >>> 0], i = r(e + 4, n * t, !0);
				return Q(e), i;
			},
			toWireType: (e, r) => {
				typeof r != "string" && K(`Cannot pass non-string to C++ string type ${n}`);
				var o = a(r), s = bi(4 + o + t);
				return O[s >>> 2 >>> 0] = o / t, i(r, s + 4, o + t), e !== null && e.push(Q, s), s;
			},
			readValueFromPointer: wt,
			destructorFunction(e) {
				Q(e);
			}
		});
	}
	function nr(e, t, n, r, i, a) {
		e >>>= 0, t >>>= 0, n >>>= 0, r >>>= 0, i >>>= 0, a >>>= 0, St[e] = {
			name: W(t),
			rawConstructor: Y(n, r),
			rawDestructor: Y(i, a),
			fields: []
		};
	}
	function rr(e, t, n, r, i, a, o, s, c, l) {
		e >>>= 0, t >>>= 0, n >>>= 0, r >>>= 0, i >>>= 0, a >>>= 0, o >>>= 0, s >>>= 0, c >>>= 0, l >>>= 0, St[e].fields.push({
			fieldName: W(t),
			getterReturnType: n,
			getter: Y(r, i),
			getterContext: a,
			setterArgumentType: o,
			setter: Y(s, c),
			setterContext: l
		});
	}
	var ir = function(e, t) {
		e >>>= 0, t >>>= 0, t = W(t), Mt(e, {
			isVoid: !0,
			name: t,
			fromWireType: () => void 0,
			toWireType: (e, t) => void 0
		});
	}, ar = () => {};
	function or(e) {
		return e >>>= 0, e ? -52 : 0;
	}
	var sr = () => {
		throw new ee();
	}, cr = [], lr = (e) => {
		var t = cr.length;
		return cr.push(e), t;
	}, ur = (e, t) => {
		for (var n = Array(e), r = 0; r < e; ++r) n[r] = Mn(O[t + r * 4 >>> 2 >>> 0], `parameter ${r}`);
		return n;
	}, dr = (e, t, n) => {
		var r = [], i = e(r, n);
		return r.length && (O[t >>> 2 >>> 0] = Z.toHandle(r)), i;
	}, fr = {}, pr = (e) => {
		var t = fr[e];
		return t === void 0 ? W(e) : t;
	}, mr = function(e, t, n) {
		t >>>= 0;
		var r = 8, [i, ...a] = ur(e, t), o = i.toWireType.bind(i), s = a.map((e) => e.readValueFromPointer.bind(e));
		e--;
		var c = Array(e);
		return lr(Yt(`methodCaller<(${a.map((e) => e.name)}) => ${i.name}>`, (t, i, a, l) => {
			for (var u = 0, d = 0; d < e; ++d) c[d] = s[d](l + u), u += r;
			var f;
			switch (n) {
				case 0:
					f = Z.toValue(t).apply(null, c);
					break;
				case 2:
					f = Reflect.construct(Z.toValue(t), c);
					break;
				case 3:
					f = c[0];
					break;
				case 1: f = Z.toValue(t)[pr(i)](...c);
			}
			return dr(o, a, f);
		}));
	};
	function hr(e) {
		e >>>= 0, e > 9 && (X[e + 1] += 1);
	}
	function gr(e, t, n, r, i) {
		return e >>>= 0, t >>>= 0, n >>>= 0, r >>>= 0, i >>>= 0, cr[e](t, n, r, i);
	}
	function _r() {
		return Z.toHandle([]);
	}
	function vr(e) {
		return e >>>= 0, Z.toHandle(pr(e));
	}
	function yr() {
		return Z.toHandle({});
	}
	function br(e) {
		e >>>= 0, Ct(Z.toValue(e)), En(e);
	}
	function xr(e, t, n) {
		e >>>= 0, t >>>= 0, n >>>= 0, e = Z.toValue(e), t = Z.toValue(t), n = Z.toValue(n), e[t] = n;
	}
	function Sr(e, t) {
		e = w(e), t >>>= 0;
		var n = /* @__PURE__ */ new Date(e * 1e3);
		if (isNaN(n.getTime())) return 1;
		R[t >>> 2 >>> 0] = n.getUTCSeconds(), R[t + 4 >>> 2 >>> 0] = n.getUTCMinutes(), R[t + 8 >>> 2 >>> 0] = n.getUTCHours(), R[t + 12 >>> 2 >>> 0] = n.getUTCDate(), R[t + 16 >>> 2 >>> 0] = n.getUTCMonth(), R[t + 20 >>> 2 >>> 0] = n.getUTCFullYear() - 1900, R[t + 24 >>> 2 >>> 0] = n.getUTCDay();
		var r = Date.UTC(n.getUTCFullYear(), 0, 1, 0, 0, 0, 0), i = (n.getTime() - r) / 864e5 | 0;
		return R[t + 28 >>> 2 >>> 0] = i, 0;
	}
	var Cr = (e) => e % 4 == 0 && (e % 100 != 0 || e % 400 == 0), wr = [
		0,
		31,
		60,
		91,
		121,
		152,
		182,
		213,
		244,
		274,
		305,
		335
	], Tr = [
		0,
		31,
		59,
		90,
		120,
		151,
		181,
		212,
		243,
		273,
		304,
		334
	], Er = (e) => (Cr(e.getFullYear()) ? wr : Tr)[e.getMonth()] + e.getDate() - 1;
	function Dr(e, t) {
		e = w(e), t >>>= 0;
		var n = /* @__PURE__ */ new Date(e * 1e3);
		if (isNaN(n.getTime())) return 1;
		R[t >>> 2 >>> 0] = n.getSeconds(), R[t + 4 >>> 2 >>> 0] = n.getMinutes(), R[t + 8 >>> 2 >>> 0] = n.getHours(), R[t + 12 >>> 2 >>> 0] = n.getDate(), R[t + 16 >>> 2 >>> 0] = n.getMonth(), R[t + 20 >>> 2 >>> 0] = n.getFullYear() - 1900, R[t + 24 >>> 2 >>> 0] = n.getDay();
		var r = Er(n) | 0;
		R[t + 28 >>> 2 >>> 0] = r, R[t + 36 >>> 2 >>> 0] = -(n.getTimezoneOffset() * 60);
		var i = new Date(n.getFullYear(), 0, 1), a = new Date(n.getFullYear(), 6, 1).getTimezoneOffset(), o = i.getTimezoneOffset(), s = (a != o && n.getTimezoneOffset() == Math.min(o, a)) | 0;
		return R[t + 32 >>> 2 >>> 0] = s, 0;
	}
	var Or = function(e) {
		e >>>= 0;
		var t = (() => {
			var t = new Date(R[e + 20 >>> 2 >>> 0] + 1900, R[e + 16 >>> 2 >>> 0], R[e + 12 >>> 2 >>> 0], R[e + 8 >>> 2 >>> 0], R[e + 4 >>> 2 >>> 0], R[e >>> 2 >>> 0], 0);
			if (isNaN(t.getTime())) return -1;
			var n = R[e + 32 >>> 2 >>> 0], r = t.getTimezoneOffset(), i = new Date(t.getFullYear(), 0, 1), a = new Date(t.getFullYear(), 6, 1).getTimezoneOffset(), o = i.getTimezoneOffset(), s = Math.min(o, a);
			if (n < 0) n = Number(a != o && s == r);
			else if (n > 0 != (s == r)) {
				var c = n > 0 ? s : Math.max(o, a);
				if (t.setTime(t.getTime() + (c - r) * 6e4), isNaN(t.getTime())) return -1;
			}
			R[e + 32 >>> 2 >>> 0] = n, R[e + 24 >>> 2 >>> 0] = t.getDay();
			var l = Er(t) | 0;
			return R[e + 28 >>> 2 >>> 0] = l, R[e >>> 2 >>> 0] = t.getSeconds(), R[e + 4 >>> 2 >>> 0] = t.getMinutes(), R[e + 8 >>> 2 >>> 0] = t.getHours(), R[e + 12 >>> 2 >>> 0] = t.getDate(), R[e + 16 >>> 2 >>> 0] = t.getMonth(), R[e + 20 >>> 2 >>> 0] = t.getYear(), t.getTime() / 1e3;
		})();
		return BigInt(t);
	};
	function kr(e, t, n, r, i, a, o) {
		e >>>= 0, i = w(i), a >>>= 0, o >>>= 0;
		try {
			var s = B.getStreamFromFD(r), c = L.mmap(s, e, i, t, n), l = c.ptr;
			return R[a >>> 2 >>> 0] = c.allocated, O[o >>> 2 >>> 0] = l, 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	function Ar(e, t, n, r, i, a) {
		e >>>= 0, t >>>= 0, a = w(a);
		try {
			var o = B.getStreamFromFD(i);
			n & 2 && B.doMsync(e, o, t, r, a);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return -e.errno;
		}
	}
	var jr = function(e, t, n, r) {
		e >>>= 0, t >>>= 0, n >>>= 0, r >>>= 0;
		var i = (/* @__PURE__ */ new Date()).getFullYear(), a = new Date(i, 0, 1), o = new Date(i, 6, 1), s = a.getTimezoneOffset(), c = o.getTimezoneOffset(), l = Math.max(s, c);
		O[e >>> 2 >>> 0] = l * 60, R[t >>> 2 >>> 0] = Number(s != c);
		var u = (e) => {
			var t = e >= 0 ? "-" : "+", n = Math.abs(e);
			return `UTC${t}${String(Math.floor(n / 60)).padStart(2, "0")}${String(n % 60).padStart(2, "0")}`;
		}, d = u(s), f = u(c);
		c < s ? (ct(d, n, 17), ct(f, r, 17)) : (ct(d, r, 17), ct(f, n, 17));
	}, Mr = () => performance.now(), Nr = () => Date.now(), Pr = 1, Fr = (e) => e >= 0 && e <= 3;
	function Ir(e, t, n) {
		if (t = w(t), n >>>= 0, !Fr(e)) return 28;
		var r;
		if (e === 0) r = Nr();
		else if (Pr) r = Mr();
		else return 52;
		var i = Math.round(r * 1e3 * 1e3);
		return z[n >>> 3 >>> 0] = BigInt(i), 0;
	}
	var Lr = () => 4294901760;
	function Rr() {
		return Lr();
	}
	var zr = (e) => {
		var t = (e - ki.buffer.byteLength + 65535) / 65536 | 0;
		try {
			return ki.grow(t), ne(), 1;
		} catch {}
	};
	function Br(e) {
		e >>>= 0;
		var t = P.length, n = Lr();
		if (e > n) return !1;
		for (var r = 1; r <= 4; r *= 2) {
			var i = t * (1 + .2 / r);
			if (i = Math.min(i, e + 100663296), zr(Math.min(n, Le(Math.max(e, i), 65536)))) return !0;
		}
		return !1;
	}
	var Vr = {}, Hr = () => i, Ur = () => {
		if (!Ur.strings) {
			var e = {
				USER: "web_user",
				LOGNAME: "web_user",
				PATH: "/",
				PWD: "/",
				HOME: "/home/web_user",
				LANG: (globalThis.navigator?.language ?? "C").replace("-", "_") + ".UTF-8",
				_: Hr()
			};
			for (var t in Vr) Vr[t] === void 0 ? delete e[t] : e[t] = Vr[t];
			var n = [];
			for (var t in e) n.push(`${t}=${e[t]}`);
			Ur.strings = n;
		}
		return Ur.strings;
	};
	function Wr(e, t) {
		e >>>= 0, t >>>= 0;
		var n = 0, r = 0;
		for (var i of Ur()) {
			var a = t + n;
			O[e + r >>> 2 >>> 0] = a, n += ct(i, a, Infinity) + 1, r += 4;
		}
		return 0;
	}
	function Gr(e, t) {
		e >>>= 0, t >>>= 0;
		var n = Ur();
		O[e >>> 2 >>> 0] = n.length;
		var r = 0;
		for (var i of n) r += Me(i) + 1;
		return O[t >>> 2 >>> 0] = r, 0;
	}
	var Kr = () => !0, qr = (e) => {
		Kr() || (m = !0), a(e, new de(e));
	}, Jr = (e, t) => {
		qr(e);
	};
	function Yr(e) {
		try {
			var t = B.getStreamFromFD(e);
			return L.close(t), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function Xr(e, t) {
		t >>>= 0;
		try {
			var n = 0, r = 0, i = 0, a = B.getStreamFromFD(e), o = a.tty ? 2 : L.isDir(a.mode) ? 3 : L.isLink(a.mode) ? 7 : 4;
			return b[t >>> 0] = o, U[t + 2 >>> 1 >>> 0] = i, z[t + 8 >>> 3 >>> 0] = BigInt(n), z[t + 16 >>> 3 >>> 0] = BigInt(r), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	var Zr = (e, t, n, r) => {
		for (var i = 0, a = 0; a < n; a++) {
			var o = O[t >>> 2 >>> 0], s = O[t + 4 >>> 2 >>> 0];
			t += 8;
			try {
				var c = L.read(e, b, o, s, r);
			} catch (e) {
				if (i > 0 && e instanceof L.ErrnoError && (e.errno == 6 || e.errno == 6)) break;
				throw e;
			}
			if (c < 0) return -1;
			if (i += c, c < s) break;
			r !== void 0 && (r += c);
		}
		return i;
	};
	function Qr(e, t, n, r, i) {
		t >>>= 0, n >>>= 0, r = w(r), i >>>= 0;
		try {
			if (isNaN(r)) return 22;
			var a = Zr(B.getStreamFromFD(e), t, n, r);
			return O[i >>> 2 >>> 0] = a, 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function $r(e, t, n, r) {
		t >>>= 0, n >>>= 0, r >>>= 0;
		try {
			var i = Zr(B.getStreamFromFD(e), t, n);
			return O[r >>> 2 >>> 0] = i, 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ei(e, t, n, r) {
		t = w(t), r >>>= 0;
		try {
			if (isNaN(t)) return 22;
			var i = B.getStreamFromFD(e);
			return L.llseek(i, t, n), z[r >>> 3 >>> 0] = BigInt(i.position), i.getdents && !t && n === 0 && (i.getdents = null), 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ti(e) {
		try {
			var t = B.getStreamFromFD(e);
			return t.stream_ops?.fsync?.(t);
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	var ni = (e, t, n, r) => {
		if (n == 1) return L.write(e, b, O[t >>> 2 >>> 0], O[t + 4 >>> 2 >>> 0], r);
		for (var i = 0, a = 0, o = t; a < n; a++, o += 8) i += O[o + 4 >>> 2 >>> 0];
		for (var s = new Uint8Array(i), c = 0, a = 0; a < n; a++, t += 8) {
			var l = O[t >>> 2 >>> 0], u = O[t + 4 >>> 2 >>> 0];
			s.set(P.subarray(l >>> 0, l + u >>> 0), c), c += u;
		}
		return L.write(e, s, 0, i, r);
	};
	function ri(e, t, n, r) {
		t >>>= 0, n >>>= 0, r >>>= 0;
		try {
			var i = ni(B.getStreamFromFD(e), t, n);
			return O[r >>> 2 >>> 0] = i, 0;
		} catch (e) {
			if (L === void 0 || e.name !== "ErrnoError") throw e;
			return e.errno;
		}
	}
	function ii(e) {
		return e >>>= 0, e;
	}
	function ai(e, t) {
		return e >>>= 0, t >>>= 0, De(P.subarray(e >>> 0, e + t >>> 0));
	}
	var oi = (e, t) => {
		if (si) for (var n = e; n < e + t; n++) {
			var r = T(n);
			r && si.set(r, n);
		}
	}, si, ci = (e) => (si || (si = /* @__PURE__ */ new WeakMap(), oi(0, Ai.length)), si.get(e) || 0), li = [], ui = () => li.length ? li.pop() : Ai.grow(1), di = (e, t) => {
		Ai.set(e, t), pe[e] = Ai.get(e);
	}, fi = (e) => {
		let t = e.length;
		return [
			t % 128 | 128,
			t >> 7,
			...e
		];
	}, pi = {
		i: 127,
		p: 127,
		j: 126,
		f: 125,
		d: 124,
		e: 111
	}, mi = (e) => fi(Array.from(e, (e) => pi[e])), hi = (e, t) => {
		var n = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0, 1, ...fi([
			1,
			96,
			...mi(t.slice(1)),
			...mi(t[0] === "v" ? "" : t[0])
		]), 2, 7, 1, 1, 101, 1, 102, 0, 0, 7, 5, 1, 1, 102, 0, 0), r = new WebAssembly.Module(n);
		return new WebAssembly.Instance(r, { e: { f: e } }).exports.f;
	}, gi = (e, t) => {
		var n = ci(e);
		if (n) return n;
		var r = ui();
		try {
			di(r, e);
		} catch (n) {
			if (!(n instanceof TypeError)) throw n;
			di(r, hi(e, t));
		}
		return si.set(e, r), r;
	};
	function _i(e, t = "i8") {
		switch (t.endsWith("*") && (t = "*"), t) {
			case "i1": return b[e >>> 0];
			case "i8": return b[e >>> 0];
			case "i16": return U[e >>> 1 >>> 0];
			case "i32": return R[e >>> 2 >>> 0];
			case "i64": return z[e >>> 3 >>> 0];
			case "float": return Pn[e >>> 2 >>> 0];
			case "double": return Fn[e >>> 3 >>> 0];
			case "*": return O[e >>> 2 >>> 0];
			default: v(`invalid type for getValue: ${t}`);
		}
	}
	function vi(e, t, n = "i8") {
		switch (n.endsWith("*") && (n = "*"), n) {
			case "i1":
				b[e >>> 0] = t;
				break;
			case "i8":
				b[e >>> 0] = t;
				break;
			case "i16":
				U[e >>> 1 >>> 0] = t;
				break;
			case "i32":
				R[e >>> 2 >>> 0] = t;
				break;
			case "i64":
				z[e >>> 3 >>> 0] = BigInt(t);
				break;
			case "float":
				Pn[e >>> 2 >>> 0] = t;
				break;
			case "double":
				Fn[e >>> 3 >>> 0] = t;
				break;
			case "*":
				O[e >>> 2 >>> 0] = t;
				break;
			default: v(`invalid type for setValue: ${n}`);
		}
	}
	L.createPreloadedFile = $e, L.preloadFile = Qe, L.staticInit(), qt(), hn(), t.wasmBinary && (p = t.wasmBinary), t.addFunction = gi, t.setValue = vi, t.getValue = _i, t.UTF8ToString = et, t.stringToUTF8 = ct, t.lengthBytesUTF8 = Me, t.FS = L;
	var yi, bi, Q, xi, $, Si, Ci, wi, Ti, Ei, Di, Oi, ki, Ai;
	function ji(e) {
		yi = e.rb, t._MagickColor_Create = e.sb, t._MagickColor_Dispose = e.tb, t._MagickColor_Count_Get = e.ub, t._MagickColor_Red_Get = e.vb, t._MagickColor_Red_Set = e.wb, t._MagickColor_Green_Get = e.xb, t._MagickColor_Green_Set = e.yb, t._MagickColor_Blue_Get = e.zb, t._MagickColor_Blue_Set = e.Ab, t._MagickColor_Alpha_Get = e.Bb, t._MagickColor_Alpha_Set = e.Cb, t._MagickColor_Black_Get = e.Db, t._MagickColor_Black_Set = e.Eb, t._MagickColor_IsCMYK_Get = e.Fb, t._MagickColor_IsCMYK_Set = e.Gb, t._MagickColor_Clone = e.Hb, t._MagickColor_FuzzyEquals = e.Ib, t._MagickColor_Initialize = e.Jb, t._MagickColorCollection_Create = e.Lb, t._MagickColorCollection_Dispose = e.Mb, t._MagickColorCollection_Get = e.Nb, t._MagickColorCollection_Set = e.Ob, t._DrawingWand_Create = e.Pb, t._DrawingWand_Dispose = e.Qb, t._DrawingWand_Affine = e.Rb, t._DrawingWand_Alpha = e.Sb, t._DrawingWand_Arc = e.Tb, t._DrawingWand_Bezier = e.Ub, t._DrawingWand_BorderColor = e.Vb, t._DrawingWand_Circle = e.Wb, t._DrawingWand_ClipPath = e.Xb, t._DrawingWand_ClipRule = e.Yb, t._DrawingWand_ClipUnits = e.Zb, t._DrawingWand_Color = e._b, t._DrawingWand_Composite = e.$b, t._DrawingWand_Density = e.ac, t._DrawingWand_Ellipse = e.bc, t._DrawingWand_FillColor = e.cc, t._DrawingWand_FillOpacity = e.dc, t._DrawingWand_FillPatternUrl = e.ec, t._DrawingWand_FillRule = e.fc, t._DrawingWand_Font = e.gc, t._DrawingWand_FontFamily = e.hc, t._DrawingWand_FontPointSize = e.ic, t._DrawingWand_FontTypeMetrics = e.jc, t._TypeMetric_Create = e.kc, t._DrawingWand_Gravity = e.lc, t._DrawingWand_Line = e.mc, t._DrawingWand_PathArcAbs = e.nc, t._DrawingWand_PathArcRel = e.oc, t._DrawingWand_PathClose = e.pc, t._DrawingWand_PathCurveToAbs = e.qc, t._DrawingWand_PathCurveToRel = e.rc, t._DrawingWand_PathFinish = e.sc, t._DrawingWand_PathLineToAbs = e.tc, t._DrawingWand_PathLineToHorizontalAbs = e.uc, t._DrawingWand_PathLineToHorizontalRel = e.vc, t._DrawingWand_PathLineToRel = e.wc, t._DrawingWand_PathLineToVerticalAbs = e.xc, t._DrawingWand_PathLineToVerticalRel = e.yc, t._DrawingWand_PathMoveToAbs = e.zc, t._DrawingWand_PathMoveToRel = e.Ac, t._DrawingWand_PathQuadraticCurveToAbs = e.Bc, t._DrawingWand_PathQuadraticCurveToRel = e.Cc, t._DrawingWand_PathSmoothCurveToAbs = e.Dc, t._DrawingWand_PathSmoothCurveToRel = e.Ec, t._DrawingWand_PathSmoothQuadraticCurveToAbs = e.Fc, t._DrawingWand_PathSmoothQuadraticCurveToRel = e.Gc, t._DrawingWand_PathStart = e.Hc, t._DrawingWand_Point = e.Ic, t._DrawingWand_Polygon = e.Jc, t._DrawingWand_Polyline = e.Kc, t._DrawingWand_PopClipPath = e.Lc, t._DrawingWand_PopGraphicContext = e.Mc, t._DrawingWand_PopPattern = e.Nc, t._DrawingWand_PushClipPath = e.Oc, t._DrawingWand_PushGraphicContext = e.Pc, t._DrawingWand_PushPattern = e.Qc, t._DrawingWand_Rectangle = e.Rc, t._DrawingWand_Render = e.Sc, t._DrawingWand_Rotation = e.Tc, t._DrawingWand_RoundRectangle = e.Uc, t._DrawingWand_Scaling = e.Vc, t._DrawingWand_SkewX = e.Wc, t._DrawingWand_SkewY = e.Xc, t._DrawingWand_StrokeAntialias = e.Yc, t._DrawingWand_StrokeColor = e.Zc, t._DrawingWand_StrokeDashArray = e._c, t._DrawingWand_StrokeDashOffset = e.$c, t._DrawingWand_StrokeLineCap = e.ad, t._DrawingWand_StrokeLineJoin = e.bd, t._DrawingWand_StrokeMiterLimit = e.cd, t._DrawingWand_StrokeOpacity = e.dd, t._DrawingWand_StrokePatternUrl = e.ed, t._DrawingWand_StrokeWidth = e.fd, t._DrawingWand_Text = e.gd, t._DrawingWand_TextAlignment = e.hd, t._DrawingWand_TextAntialias = e.id, t._DrawingWand_TextDecoration = e.jd, t._DrawingWand_TextDirection = e.kd, t._DrawingWand_TextEncoding = e.ld, t._DrawingWand_TextInterlineSpacing = e.md, t._DrawingWand_TextInterwordSpacing = e.nd, t._DrawingWand_TextKerning = e.od, t._DrawingWand_TextUnderColor = e.pd, t._DrawingWand_Translation = e.qd, t._DrawingWand_Viewbox = e.rd, t._MagickExceptionHelper_Description = e.sd, t._MagickExceptionHelper_Dispose = e.td, t._MagickExceptionHelper_Related = e.ud, t._MagickExceptionHelper_RelatedCount = e.vd, t._MagickExceptionHelper_Message = e.wd, t._MagickExceptionHelper_Severity = e.xd, t._PdfInfo_PageCount = e.yd, t._Environment_Initialize = e.zd, t._Environment_GetEnv = e.Ad, t._Environment_SetEnv = e.Bd, t._MagickMemory_Relinquish = e.Cd, t._Magick_Delegates_Get = e.Dd, t._Magick_Features_Get = e.Ed, t._Magick_ImageMagickVersion_Get = e.Fd, t._Magick_GetFonts = e.Gd, t._Magick_GetFontFamily = e.Hd, t._Magick_GetFontName = e.Id, t._Magick_GetWindowsResource = e.Jd, t._Magick_DisposeFonts = e.Kd, t._Magick_ResetRandomSeed = e.Ld, t._Magick_SetDefaultFontFile = e.Md, t._Magick_SetRandomSeed = e.Nd, t._Magick_SetLogDelegate = e.Od, t._Magick_SetLogEvents = e.Pd, t._MagickFormatInfo_CreateList = e.Qd, t._MagickFormatInfo_DisposeList = e.Rd, t._MagickFormatInfo_CanReadMultithreaded_Get = e.Sd, t._MagickFormatInfo_CanWriteMultithreaded_Get = e.Td, t._MagickFormatInfo_Description_Get = e.Ud, t._MagickFormatInfo_Format_Get = e.Vd, t._MagickFormatInfo_MimeType_Get = e.Wd, t._MagickFormatInfo_Module_Get = e.Xd, t._MagickFormatInfo_SupportsMultipleFrames_Get = e.Yd, t._MagickFormatInfo_SupportsReading_Get = e.Zd, t._MagickFormatInfo_SupportsWriting_Get = e._d, t._MagickFormatInfo_Version_Get = e.$d, t._MagickFormatInfo_GetInfo = e.ae, t._MagickFormatInfo_GetInfoByName = e.be, t._MagickFormatInfo_GetInfoWithBlob = e.ce, t._MagickFormatInfo_Unregister = e.de, t._MagickImage_Create = e.ee, t._MagickImage_Dispose = e.fe, t._MagickImage_AnimationDelay_Get = e.ge, t._MagickImage_AnimationDelay_Set = e.he, t._MagickImage_AnimationIterations_Get = e.ie, t._MagickImage_AnimationIterations_Set = e.je, t._MagickImage_AnimationTicksPerSecond_Get = e.ke, t._MagickImage_AnimationTicksPerSecond_Set = e.le, t._MagickImage_BackgroundColor_Get = e.me, t._MagickImage_BackgroundColor_Set = e.ne, t._MagickImage_BaseHeight_Get = e.oe, t._MagickImage_BaseWidth_Get = e.pe, t._MagickImage_BlackPointCompensation_Get = e.qe, t._MagickImage_BlackPointCompensation_Set = e.re, t._MagickImage_BorderColor_Get = e.se, t._MagickImage_BorderColor_Set = e.te, t._MagickImage_BoundingBox_Get = e.ue, t._MagickRectangle_Create = e.ve, t._MagickImage_ChannelCount_Get = e.we, t._MagickImage_ChromaBlue_Get = e.xe, t._PrimaryInfo_Create = e.ye, t._MagickImage_ChromaBlue_Set = e.ze, t._MagickImage_ChromaGreen_Get = e.Ae, t._MagickImage_ChromaGreen_Set = e.Be, t._MagickImage_ChromaRed_Get = e.Ce, t._MagickImage_ChromaRed_Set = e.De, t._MagickImage_ChromaWhite_Get = e.Ee, t._MagickImage_ChromaWhite_Set = e.Fe, t._MagickImage_ClassType_Get = e.Ge, t._MagickImage_ClassType_Set = e.He, t._QuantizeSettings_Create = e.Ie, t._QuantizeSettings_Dispose = e.Je, t._MagickImage_ColorFuzz_Get = e.Ke, t._MagickImage_ColorFuzz_Set = e.Le, t._MagickImage_ColormapSize_Get = e.Me, t._MagickImage_ColormapSize_Set = e.Ne, t._MagickImage_ColorSpace_Get = e.Oe, t._MagickImage_ColorSpace_Set = e.Pe, t._MagickImage_ColorType_Get = e.Qe, t._MagickImage_ColorType_Set = e.Re, t._MagickImage_Compose_Get = e.Se, t._MagickImage_Compose_Set = e.Te, t._MagickImage_Compression_Get = e.Ue, t._MagickImage_Compression_Set = e.Ve, t._MagickImage_Depth_Get = e.We, t._MagickImage_Depth_Set = e.Xe, t._MagickImage_EncodingGeometry_Get = e.Ye, t._MagickImage_Endian_Get = e.Ze, t._MagickImage_Endian_Set = e._e, t._MagickImage_FileName_Get = e.$e, t._MagickImage_FileName_Set = e.af, t._MagickImage_FilterType_Get = e.bf, t._MagickImage_FilterType_Set = e.cf, t._MagickImage_Format_Get = e.df, t._MagickImage_Format_Set = e.ef, t._MagickImage_Gamma_Get = e.ff, t._MagickImage_GifDisposeMethod_Get = e.gf, t._MagickImage_GifDisposeMethod_Set = e.hf, t._MagickImage_HasAlpha_Get = e.jf, t._MagickImage_HasAlpha_Set = e.kf, t._MagickImage_Height_Get = e.lf, t._MagickImage_Interlace_Get = e.mf, t._MagickImage_Interlace_Set = e.nf, t._MagickImage_Interpolate_Get = e.of, t._MagickImage_Interpolate_Set = e.pf, t._MagickImage_IsOpaque_Get = e.qf, t._MagickImage_MatteColor_Get = e.rf, t._MagickImage_MatteColor_Set = e.sf, t._MagickImage_MeanErrorPerPixel_Get = e.tf, t._MagickImage_MetaChannelCount_Get = e.uf, t._MagickImage_MetaChannelCount_Set = e.vf, t._MagickImage_NormalizedMaximumError_Get = e.wf, t._MagickImage_NormalizedMeanError_Get = e.xf, t._MagickImage_Orientation_Get = e.yf, t._MagickImage_Orientation_Set = e.zf, t._MagickImage_Page_Get = e.Af, t._MagickImage_Page_Set = e.Bf, t._MagickImage_Quality_Get = e.Cf, t._MagickImage_Quality_Set = e.Df, t._MagickImage_RenderingIntent_Get = e.Ef, t._MagickImage_RenderingIntent_Set = e.Ff, t._MagickImage_ResolutionUnits_Get = e.Gf, t._MagickImage_ResolutionUnits_Set = e.Hf, t._MagickImage_ResolutionX_Get = e.If, t._MagickImage_ResolutionX_Set = e.Jf, t._MagickImage_ResolutionY_Get = e.Kf, t._MagickImage_ResolutionY_Set = e.Lf, t._MagickImage_Signature_Get = e.Mf, t._MagickImage_TotalColors_Get = e.Nf, t._MagickImage_VirtualPixelMethod_Get = e.Of, t._MagickImage_VirtualPixelMethod_Set = e.Pf, t._MagickImage_Width_Get = e.Qf, t._MagickImage_AdaptiveBlur = e.Rf, t._MagickImage_AdaptiveResize = e.Sf, t._MagickImage_AdaptiveSharpen = e.Tf, t._MagickImage_AdaptiveThreshold = e.Uf, t._MagickImage_AddNoise = e.Vf, t._MagickImage_AffineTransform = e.Wf, t._MagickImage_Annotate = e.Xf, t._MagickImage_AutoGamma = e.Yf, t._MagickImage_AutoLevel = e.Zf, t._MagickImage_AutoOrient = e._f, t._MagickImage_AutoThreshold = e.$f, t._MagickImage_BilateralBlur = e.ag, t._MagickImage_BlackThreshold = e.bg, t._MagickImage_BlueShift = e.cg, t._MagickImage_Blur = e.dg, t._MagickImage_Border = e.eg, t._MagickImage_BrightnessContrast = e.fg, t._MagickImage_CannyEdge = e.gg, t._MagickImage_ChannelOffset = e.hg, t._MagickImage_Charcoal = e.ig, t._MagickImage_Chop = e.jg, t._MagickImage_Clahe = e.kg, t._MagickImage_Clamp = e.lg, t._MagickImage_ClipPath = e.mg, t._MagickImage_Clone = e.ng, t._MagickImage_CloneArea = e.og, t._MagickImage_Clut = e.pg, t._MagickImage_ColorDecisionList = e.qg, t._MagickImage_Colorize = e.rg, t._MagickImage_ColorMatrix = e.sg, t._MagickImage_ColorThreshold = e.tg, t._MagickImage_Compare = e.ug, t._MagickImage_CompareDistortion = e.vg, t._MagickImage_Composite = e.wg, t._MagickImage_CompositeGravity = e.xg, t._MagickImage_ConnectedComponents = e.yg, t._MagickImage_Contrast = e.zg, t._MagickImage_ContrastStretch = e.Ag, t._MagickImage_ConvexHull = e.Bg, t._MagickImage_Convolve = e.Cg, t._MagickImage_CopyPixels = e.Dg, t._MagickImage_Crop = e.Eg, t._MagickImage_CropToTiles = e.Fg, t._MagickImage_CycleColormap = e.Gg, t._MagickImage_Decipher = e.Hg, t._MagickImage_Deskew = e.Ig, t._MagickImage_Despeckle = e.Jg, t._MagickImage_DetermineBitDepth = e.Kg, t._MagickImage_DetermineColorType = e.Lg, t._MagickImage_Distort = e.Mg, t._MagickImage_Edge = e.Ng, t._MagickImage_Emboss = e.Og, t._MagickImage_Encipher = e.Pg, t._MagickImage_Enhance = e.Qg, t._MagickImage_Equalize = e.Rg, t._MagickImage_Equals = e.Sg, t._MagickImage_EvaluateFunction = e.Tg, t._MagickImage_EvaluateGeometry = e.Ug, t._MagickImage_EvaluateOperator = e.Vg, t._MagickImage_Extent = e.Wg, t._MagickImage_Flip = e.Xg, t._MagickImage_FloodFill = e.Yg, t._MagickImage_Flop = e.Zg, t._MagickImage_FontTypeMetrics = e._g, t._MagickImage_FormatExpression = e.$g, t._MagickImage_Frame = e.ah, t._MagickImage_Fx = e.bh, t._MagickImage_GammaCorrect = e.ch, t._MagickImage_GaussianBlur = e.dh, t._MagickImage_GetArtifact = e.eh, t._MagickImage_GetAttribute = e.fh, t._MagickImage_GetColormapColor = e.gh, t._MagickImage_GetNext = e.hh, t._MagickImage_GetNextArtifactName = e.ih, t._MagickImage_GetNextAttributeName = e.jh, t._MagickImage_GetNextProfileName = e.kh, t._MagickImage_GetProfile = e.lh, t._MagickImage_GetReadMask = e.mh, t._MagickImage_GetWriteMask = e.nh, t._MagickImage_Grayscale = e.oh, t._MagickImage_HaldClut = e.ph, t._MagickImage_HasChannel = e.qh, t._MagickImage_HasProfile = e.rh, t._MagickImage_Histogram = e.sh, t._MagickImage_HoughLine = e.th, t._MagickImage_Implode = e.uh, t._MagickImage_ImportIndexedPixels = e.vh, t._MagickImage_ImportPixels = e.wh, t._MagickImage_Integral = e.xh, t._MagickImage_InterpolativeResize = e.yh, t._MagickImage_InverseLevel = e.zh, t._MagickImage_Kmeans = e.Ah, t._MagickImage_Kuwahara = e.Bh, t._MagickImage_Level = e.Ch, t._MagickImage_LevelColors = e.Dh, t._MagickImage_LinearStretch = e.Eh, t._MagickImage_LiquidRescale = e.Fh, t._MagickImage_LocalContrast = e.Gh, t._MagickImage_Magnify = e.Hh, t._MagickImage_MeanShift = e.Ih, t._MagickImage_Minify = e.Jh, t._MagickImage_MinimumBoundingBox = e.Kh, t._MagickImage_Modulate = e.Lh, t._MagickImage_Moments = e.Mh, t._MagickImage_Morphology = e.Nh, t._MagickImage_MotionBlur = e.Oh, t._MagickImage_Negate = e.Ph, t._MagickImage_Normalize = e.Qh, t._MagickImage_OilPaint = e.Rh, t._MagickImage_Opaque = e.Sh, t._MagickImage_OrderedDither = e.Th, t._MagickImage_Perceptible = e.Uh, t._MagickImage_PerceptualHash = e.Vh, t._MagickImage_Quantize = e.Wh, t._MagickImage_Polaroid = e.Xh, t._MagickImage_Posterize = e.Yh, t._MagickImage_RaiseOrLower = e.Zh, t._MagickImage_RandomThreshold = e._h, t._MagickImage_RangeThreshold = e.$h, t._MagickImage_ReadBlob = e.ai, t._MagickImage_ReadFile = e.bi, t._MagickImage_ReadPixels = e.ci, t._MagickImage_ReadStream = e.di, t._MagickImage_RegionMask = e.ei, t._MagickImage_Remap = e.fi, t._MagickImage_RemoveArtifact = e.gi, t._MagickImage_RemoveAttribute = e.hi, t._MagickImage_RemoveProfile = e.ii, t._MagickImage_ResetArtifactIterator = e.ji, t._MagickImage_ResetAttributeIterator = e.ki, t._MagickImage_ResetProfileIterator = e.li, t._MagickImage_Resample = e.mi, t._MagickImage_Resize = e.ni, t._MagickImage_Roll = e.oi, t._MagickImage_Rotate = e.pi, t._MagickImage_RotationalBlur = e.qi, t._MagickImage_Sample = e.ri, t._MagickImage_Scale = e.si, t._MagickImage_Segment = e.ti, t._MagickImage_SelectiveBlur = e.ui, t._MagickImage_Separate = e.vi, t._MagickImage_SepiaTone = e.wi, t._MagickImage_SetAlpha = e.xi, t._MagickImage_SetArtifact = e.yi, t._MagickImage_SetAttribute = e.zi, t._MagickImage_SetBitDepth = e.Ai, t._MagickImage_SetClientData = e.Bi, t._MagickImage_SetColormapColor = e.Ci, t._MagickImage_SetColorMetric = e.Di, t._MagickImage_SetNext = e.Ei, t._MagickImage_SetProfile = e.Fi, t._MagickImage_SetProgressDelegate = e.Gi, t._MagickImage_SetReadMask = e.Hi, t._MagickImage_SetWriteMask = e.Ii, t._MagickImage_Shade = e.Ji, t._MagickImage_Shadow = e.Ki, t._MagickImage_Sharpen = e.Li, t._MagickImage_Shave = e.Mi, t._MagickImage_Shear = e.Ni, t._MagickImage_SigmoidalContrast = e.Oi, t._MagickImage_SparseColor = e.Pi, t._MagickImage_Spread = e.Qi, t._MagickImage_Sketch = e.Ri, t._MagickImage_Solarize = e.Si, t._MagickImage_SortPixels = e.Ti, t._MagickImage_Splice = e.Ui, t._MagickImage_Statistic = e.Vi, t._MagickImage_Statistics = e.Wi, t._MagickImage_Stegano = e.Xi, t._MagickImage_Stereo = e.Yi, t._MagickImage_Strip = e.Zi, t._MagickImage_SubImageSearch = e._i, t._MagickImage_Swirl = e.$i, t._MagickImage_Texture = e.aj, t._MagickImage_Threshold = e.bj, t._MagickImage_Thumbnail = e.cj, t._MagickImage_Tint = e.dj, t._MagickImage_Transparent = e.ej, t._MagickImage_TransparentChroma = e.fj, t._MagickImage_Transpose = e.gj, t._MagickImage_Transverse = e.hj, t._MagickImage_Trim = e.ij, t._MagickImage_UniqueColors = e.jj, t._MagickImage_UnsharpMask = e.kj, t._MagickImage_Vignette = e.lj, t._MagickImage_Wave = e.mj, t._MagickImage_WaveletDenoise = e.nj, t._MagickImage_WhiteBalance = e.oj, t._MagickImage_WhiteThreshold = e.pj, t._MagickImage_WriteBlob = e.qj, t._MagickImage_WriteFile = e.rj, t._MagickImage_WriteStream = e.sj, t._MagickImageCollection_Append = e.tj, t._MagickImageCollection_Coalesce = e.uj, t._MagickImageCollection_Combine = e.vj, t._MagickImageCollection_Complex = e.wj, t._MagickImageCollection_Deconstruct = e.xj, t._MagickImageCollection_Dispose = e.yj, t._MagickImageCollection_Evaluate = e.zj, t._MagickImageCollection_Fx = e.Aj, t._MagickImageCollection_Merge = e.Bj, t._MagickImageCollection_Montage = e.Cj, t._MagickImageCollection_Morph = e.Dj, t._MagickImageCollection_Optimize = e.Ej, t._MagickImageCollection_OptimizePlus = e.Fj, t._MagickImageCollection_OptimizeTransparency = e.Gj, t._MagickImageCollection_Polynomial = e.Hj, t._MagickImageCollection_Quantize = e.Ij, t._MagickImageCollection_ReadBlob = e.Jj, t._MagickImageCollection_ReadFile = e.Kj, t._MagickImageCollection_ReadStream = e.Lj, t._MagickImageCollection_Remap = e.Mj, t._MagickImageCollection_Smush = e.Nj, t._MagickImageCollection_WriteFile = e.Oj, t._MagickImageCollection_WriteStream = e.Pj, t._DoubleMatrix_Create = e.Qj, t._DoubleMatrix_Dispose = e.Rj, t._OpenCL_GetDevices = e.Sj, t._OpenCL_GetDevice = e.Tj, t._OpenCL_GetEnabled = e.Uj, t._OpenCL_SetEnabled = e.Vj, t._OpenCLDevice_DeviceType_Get = e.Wj, t._OpenCLDevice_BenchmarkScore_Get = e.Xj, t._OpenCLDevice_IsEnabled_Get = e.Yj, t._OpenCLDevice_IsEnabled_Set = e.Zj, t._OpenCLDevice_Name_Get = e._j, t._OpenCLDevice_Version_Get = e.$j, t._OpenCLDevice_GetKernelProfileRecords = e.ak, t._OpenCLDevice_GetKernelProfileRecord = e.bk, t._OpenCLDevice_SetProfileKernels = e.ck, t._OpenCLKernelProfileRecord_Count_Get = e.dk, t._OpenCLKernelProfileRecord_Name_Get = e.ek, t._OpenCLKernelProfileRecord_MaximumDuration_Get = e.fk, t._OpenCLKernelProfileRecord_MinimumDuration_Get = e.gk, t._OpenCLKernelProfileRecord_TotalDuration_Get = e.hk, t._JpegOptimizer_CompressFile = e.ik, t._JpegOptimizer_CompressStream = e.jk, bi = t._malloc = e.kk, Q = t._free = e.lk, t._PixelCollection_Create = e.mk, t._PixelCollection_Dispose = e.nk, t._PixelCollection_GetArea = e.ok, t._PixelCollection_GetReadOnlyArea = e.pk, t._PixelCollection_SetArea = e.qk, t._PixelCollection_ToByteArray = e.rk, t._PixelCollection_ToShortArray = e.sk, t._Quantum_Depth_Get = e.tk, t._Quantum_Max_Get = e.uk, t._ResourceLimits_Area_Get = e.vk, t._ResourceLimits_Area_Set = e.wk, t._ResourceLimits_Disk_Get = e.xk, t._ResourceLimits_Disk_Set = e.yk, t._ResourceLimits_Height_Get = e.zk, t._ResourceLimits_Height_Set = e.Ak, t._ResourceLimits_ListLength_Get = e.Bk, t._ResourceLimits_ListLength_Set = e.Ck, t._ResourceLimits_MaxMemoryRequest_Get = e.Dk, t._ResourceLimits_MaxMemoryRequest_Set = e.Ek, t._ResourceLimits_MaxProfileSize_Get = e.Fk, t._ResourceLimits_MaxProfileSize_Set = e.Gk, t._ResourceLimits_Memory_Get = e.Hk, t._ResourceLimits_Memory_Set = e.Ik, t._ResourceLimits_Thread_Get = e.Jk, t._ResourceLimits_Thread_Set = e.Kk, t._ResourceLimits_Throttle_Get = e.Lk, t._ResourceLimits_Throttle_Set = e.Mk, t._ResourceLimits_Time_Get = e.Nk, t._ResourceLimits_Time_Set = e.Ok, t._ResourceLimits_Width_Get = e.Pk, t._ResourceLimits_Width_Set = e.Qk, t._ResourceLimits_LimitMemory = e.Rk, t._ResourceLimits_TrimMemory = e.Sk, t._DrawingSettings_Create = e.Tk, t._DrawingSettings_Dispose = e.Uk, t._DrawingSettings_BorderColor_Get = e.Vk, t._DrawingSettings_BorderColor_Set = e.Wk, t._DrawingSettings_FillColor_Get = e.Xk, t._DrawingSettings_FillColor_Set = e.Yk, t._DrawingSettings_FillRule_Get = e.Zk, t._DrawingSettings_FillRule_Set = e._k, t._DrawingSettings_Font_Get = e.$k, t._DrawingSettings_Font_Set = e.al, t._DrawingSettings_FontFamily_Get = e.bl, t._DrawingSettings_FontFamily_Set = e.cl, t._DrawingSettings_FontPointsize_Get = e.dl, t._DrawingSettings_FontPointsize_Set = e.el, t._DrawingSettings_FontStyle_Get = e.fl, t._DrawingSettings_FontStyle_Set = e.gl, t._DrawingSettings_FontWeight_Get = e.hl, t._DrawingSettings_FontWeight_Set = e.il, t._DrawingSettings_StrokeAntiAlias_Get = e.jl, t._DrawingSettings_StrokeAntiAlias_Set = e.kl, t._DrawingSettings_StrokeColor_Get = e.ll, t._DrawingSettings_StrokeColor_Set = e.ml, t._DrawingSettings_StrokeDashOffset_Get = e.nl, t._DrawingSettings_StrokeDashOffset_Set = e.ol, t._DrawingSettings_StrokeLineCap_Get = e.pl, t._DrawingSettings_StrokeLineCap_Set = e.ql, t._DrawingSettings_StrokeLineJoin_Get = e.rl, t._DrawingSettings_StrokeLineJoin_Set = e.sl, t._DrawingSettings_StrokeMiterLimit_Get = e.tl, t._DrawingSettings_StrokeMiterLimit_Set = e.ul, t._DrawingSettings_StrokeWidth_Get = e.vl, t._DrawingSettings_StrokeWidth_Set = e.wl, t._DrawingSettings_TextAntiAlias_Get = e.xl, t._DrawingSettings_TextAntiAlias_Set = e.yl, t._DrawingSettings_TextDirection_Get = e.zl, t._DrawingSettings_TextDirection_Set = e.Al, t._DrawingSettings_TextEncoding_Get = e.Bl, t._DrawingSettings_TextEncoding_Set = e.Cl, t._DrawingSettings_TextGravity_Get = e.Dl, t._DrawingSettings_TextGravity_Set = e.El, t._DrawingSettings_TextInterlineSpacing_Get = e.Fl, t._DrawingSettings_TextInterlineSpacing_Set = e.Gl, t._DrawingSettings_TextInterwordSpacing_Get = e.Hl, t._DrawingSettings_TextInterwordSpacing_Set = e.Il, t._DrawingSettings_TextKerning_Get = e.Jl, t._DrawingSettings_TextKerning_Set = e.Kl, t._DrawingSettings_TextUnderColor_Get = e.Ll, t._DrawingSettings_TextUnderColor_Set = e.Ml, t._DrawingSettings_SetAffine = e.Nl, t._DrawingSettings_SetFillPattern = e.Ol, t._DrawingSettings_SetStrokeDashArray = e.Pl, t._DrawingSettings_SetStrokePattern = e.Ql, t._DrawingSettings_SetText = e.Rl, t._MagickSettings_Create = e.Sl, t._MagickSettings_Dispose = e.Tl, t._MagickSettings_AntiAlias_Get = e.Ul, t._MagickSettings_AntiAlias_Set = e.Vl, t._MagickSettings_BackgroundColor_Get = e.Wl, t._MagickSettings_BackgroundColor_Set = e.Xl, t._MagickSettings_ColorSpace_Get = e.Yl, t._MagickSettings_ColorSpace_Set = e.Zl, t._MagickSettings_ColorType_Get = e._l, t._MagickSettings_ColorType_Set = e.$l, t._MagickSettings_Compression_Get = e.am, t._MagickSettings_Compression_Set = e.bm, t._MagickSettings_Debug_Get = e.cm, t._MagickSettings_Debug_Set = e.dm, t._MagickSettings_Density_Get = e.em, t._MagickSettings_Density_Set = e.fm, t._MagickSettings_Depth_Get = e.gm, t._MagickSettings_Depth_Set = e.hm, t._MagickSettings_Endian_Get = e.im, t._MagickSettings_Endian_Set = e.jm, t._MagickSettings_Extract_Get = e.km, t._MagickSettings_Extract_Set = e.lm, t._MagickSettings_Format_Get = e.mm, t._MagickSettings_Format_Set = e.nm, t._MagickSettings_FontPointsize_Get = e.om, t._MagickSettings_FontPointsize_Set = e.pm, t._MagickSettings_Interlace_Get = e.qm, t._MagickSettings_Interlace_Set = e.rm, t._MagickSettings_Monochrome_Get = e.sm, t._MagickSettings_Monochrome_Set = e.tm, t._MagickSettings_Verbose_Get = e.um, t._MagickSettings_Verbose_Set = e.vm, t._MagickSettings_SetColorFuzz = e.wm, t._MagickSettings_SetFileName = e.xm, t._MagickSettings_SetFont = e.ym, t._MagickSettings_SetNumberScenes = e.zm, t._MagickSettings_SetOption = e.Am, t._MagickSettings_SetPage = e.Bm, t._MagickSettings_SetPing = e.Cm, t._MagickSettings_SetQuality = e.Dm, t._MagickSettings_SetScenes = e.Em, t._MagickSettings_SetScene = e.Fm, t._MagickSettings_SetSize = e.Gm, t._MontageSettings_Create = e.Hm, t._MontageSettings_Dispose = e.Im, t._MontageSettings_SetBackgroundColor = e.Jm, t._MontageSettings_SetBorderColor = e.Km, t._MontageSettings_SetBorderWidth = e.Lm, t._MontageSettings_SetFillColor = e.Mm, t._MontageSettings_SetFont = e.Nm, t._MontageSettings_SetFontPointsize = e.Om, t._MontageSettings_SetFrameGeometry = e.Pm, t._MontageSettings_SetGeometry = e.Qm, t._MontageSettings_SetGravity = e.Rm, t._MontageSettings_SetShadow = e.Sm, t._MontageSettings_SetStrokeColor = e.Tm, t._MontageSettings_SetTextureFileName = e.Um, t._MontageSettings_SetTileGeometry = e.Vm, t._MontageSettings_SetTitle = e.Wm, t._QuantizeSettings_SetColors = e.Xm, t._QuantizeSettings_SetColorSpace = e.Ym, t._QuantizeSettings_SetDitherMethod = e.Zm, t._QuantizeSettings_SetMeasureErrors = e._m, t._QuantizeSettings_SetTreeDepth = e.$m, t._ChannelMoments_Centroid_Get = e.an, t._ChannelMoments_EllipseAngle_Get = e.bn, t._ChannelMoments_EllipseAxis_Get = e.cn, t._ChannelMoments_EllipseEccentricity_Get = e.dn, t._ChannelMoments_EllipseIntensity_Get = e.en, t._ChannelMoments_GetHuInvariants = e.fn, t._ChannelPerceptualHash_GetHuPhash = e.gn, t._ChannelStatistics_Depth_Get = e.hn, t._ChannelStatistics_Entropy_Get = e.jn, t._ChannelStatistics_Kurtosis_Get = e.kn, t._ChannelStatistics_Maximum_Get = e.ln, t._ChannelStatistics_Mean_Get = e.mn, t._ChannelStatistics_Minimum_Get = e.nn, t._ChannelStatistics_Skewness_Get = e.on, t._ChannelStatistics_StandardDeviation_Get = e.pn, t._Moments_DisposeList = e.qn, t._Moments_GetInstance = e.rn, t._PerceptualHash_DisposeList = e.sn, t._PerceptualHash_GetInstance = e.tn, t._Statistics_DisposeList = e.un, t._Statistics_GetInstance = e.vn, t._ConnectedComponent_DisposeList = e.wn, t._ConnectedComponent_GetArea = e.xn, t._ConnectedComponent_GetCentroid = e.yn, t._ConnectedComponent_GetColor = e.zn, t._ConnectedComponent_GetHeight = e.An, t._ConnectedComponent_GetId = e.Bn, t._ConnectedComponent_GetWidth = e.Cn, t._ConnectedComponent_GetX = e.Dn, t._ConnectedComponent_GetY = e.En, t._ConnectedComponent_GetInstance = e.Fn, t._MagickGeometry_Create = e.Gn, t._MagickGeometry_Dispose = e.Hn, t._MagickGeometry_X_Get = e.In, t._MagickGeometry_Y_Get = e.Jn, t._MagickGeometry_Width_Get = e.Kn, t._MagickGeometry_Height_Get = e.Ln, t._MagickGeometry_Initialize = e.Mn, t._MagickRectangle_Dispose = e.Nn, t._MagickRectangle_X_Get = e.On, t._MagickRectangle_X_Set = e.Pn, t._MagickRectangle_Y_Get = e.Qn, t._MagickRectangle_Y_Set = e.Rn, t._MagickRectangle_Width_Get = e.Sn, t._MagickRectangle_Width_Set = e.Tn, t._MagickRectangle_Height_Get = e.Un, t._MagickRectangle_Height_Set = e.Vn, t._MagickRectangle_FromPageSize = e.Wn, t._OffsetInfo_Create = e.Xn, t._OffsetInfo_Dispose = e.Yn, t._OffsetInfo_SetX = e.Zn, t._OffsetInfo_SetY = e._n, t._PointInfo_X_Get = e.$n, t._PointInfo_Y_Get = e.ao, t._PointInfoCollection_Create = e.bo, t._PointInfoCollection_Dispose = e.co, t._PointInfoCollection_GetX = e.eo, t._PointInfoCollection_GetY = e.fo, t._PointInfoCollection_Set = e.go, t._PrimaryInfo_Dispose = e.ho, t._PrimaryInfo_X_Get = e.io, t._PrimaryInfo_X_Set = e.jo, t._PrimaryInfo_Y_Get = e.ko, t._PrimaryInfo_Y_Set = e.lo, t._PrimaryInfo_Z_Get = e.mo, t._PrimaryInfo_Z_Set = e.no, t._StringInfo_Length_Get = e.oo, t._StringInfo_Datum_Get = e.po, t._TypeMetric_Dispose = e.qo, t._TypeMetric_Ascent_Get = e.ro, t._TypeMetric_Descent_Get = e.so, t._TypeMetric_MaxHorizontalAdvance_Get = e.to, t._TypeMetric_TextHeight_Get = e.uo, t._TypeMetric_TextWidth_Get = e.vo, t._TypeMetric_UnderlinePosition_Get = e.wo, t._TypeMetric_UnderlineThickness_Get = e.xo, xi = e.yo, $ = e.zo, Si = e.Ao, Ci = e.Bo, wi = e.Co, Ti = e.Do, Ei = e.Eo, Di = e.Fo, Oi = e.Go, ki = e.pb, Ai = e.Kb;
	}
	var Mi = {
		kb: E,
		s: ge,
		v: _e,
		e: k,
		l: xe,
		Na: Se,
		ba: Ce,
		b: we,
		ab: Te,
		i: Ee,
		Ga: V,
		Ea: tt,
		Ha: nt,
		nb: rt,
		Da: H,
		U: ot,
		Ca: st,
		wa: lt,
		jb: ut,
		Ka: dt,
		za: ft,
		Aa: pt,
		S: mt,
		ib: ht,
		gb: gt,
		hb: _t,
		Ba: vt,
		fb: yt,
		oa: bt,
		Ia: xt,
		Wa: jt,
		ea: Ft,
		Ta: It,
		Z: Cn,
		Ra: On,
		F: jn,
		g: Nn,
		da: Ln,
		p: Wn,
		D: Gn,
		u: Kn,
		Sa: qn,
		W: tr,
		Xa: nr,
		Y: rr,
		Ua: ir,
		lb: ar,
		db: or,
		bb: sr,
		y: mr,
		t: En,
		Va: hr,
		x: gr,
		X: _r,
		G: vr,
		P: yr,
		w: br,
		E: xr,
		qa: Sr,
		ra: Dr,
		sa: Or,
		ob: kr,
		pa: Ar,
		ta: jr,
		Fa: Ir,
		$: Nr,
		eb: Rr,
		R: Mr,
		cb: Br,
		La: Wr,
		Ma: Gr,
		H: Jr,
		K: Yr,
		_: Xr,
		mb: Qr,
		aa: $r,
		ua: ei,
		ya: ti,
		T: ri,
		_a: Xi,
		ka: _a,
		la: ga,
		r: Ki,
		a: Ii,
		c: Fi,
		h: Pi,
		k: Bi,
		$a: ha,
		I: Yi,
		d: Wi,
		N: Qi,
		q: Ui,
		Za: $i,
		Ya: na,
		L: sa,
		ma,
		Qa: ca,
		C: Gi,
		ja: qi,
		O: fa,
		V: la,
		z: Hi,
		xa: Vi,
		o: oa,
		m: Li,
		f: Ri,
		ha: ea,
		ga: ta,
		n: Ni,
		j: zi,
		B: Zi,
		M: ia,
		A: aa,
		Pa: ua,
		fa: ra,
		J: va,
		Oa: da,
		Q: ya,
		na: pa,
		ia: Ji,
		ca: ii,
		Ja: qr,
		va: ai
	};
	function Ni(e, t, n, r) {
		var i = S();
		try {
			T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Pi(e, t, n, r) {
		var i = S();
		try {
			return T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Fi(e, t, n) {
		var r = S();
		try {
			return T(e)(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ii(e, t) {
		var n = S();
		try {
			return T(e)(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Li(e, t) {
		var n = S();
		try {
			T(e)(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ri(e, t, n) {
		var r = S();
		try {
			T(e)(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function zi(e, t, n, r, i) {
		var a = S();
		try {
			T(e)(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Bi(e, t, n, r, i) {
		var a = S();
		try {
			return T(e)(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Vi(e, t, n, r) {
		var i = S();
		try {
			return T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Hi(e, t) {
		var n = S();
		try {
			return T(e)(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			return $(1, 0), 0n;
		}
	}
	function Ui(e, t, n, r, i, a, o, s, c) {
		var l = S();
		try {
			return T(e)(t, n, r, i, a, o, s, c);
		} catch (e) {
			if (x(l), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Wi(e, t, n, r, i, a, o) {
		var s = S();
		try {
			return T(e)(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Gi(e, t, n, r, i) {
		var a = S();
		try {
			return T(e)(t, n, r, i);
		} catch (e) {
			if (x(a), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ki(e) {
		var t = S();
		try {
			return T(e)();
		} catch (e) {
			if (x(t), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function qi(e, t, n) {
		var r = S();
		try {
			return T(e)(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Ji(e, t, n) {
		var r = S();
		try {
			T(e)(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Yi(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Xi(e, t, n) {
		var r = S();
		try {
			return T(e)(t, n);
		} catch (e) {
			if (x(r), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Zi(e, t, n, r, i, a) {
		var o = S();
		try {
			T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function Qi(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			return T(e)(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function $i(e, t, n, r, i, a, o, s, c, l) {
		var u = S();
		try {
			return T(e)(t, n, r, i, a, o, s, c, l);
		} catch (e) {
			if (x(u), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ea(e, t, n, r) {
		var i = S();
		try {
			T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ta(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			T(e)(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function na(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			return T(e)(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ra(e, t, n, r, i, a, o, s, c, l) {
		var u = S();
		try {
			T(e)(t, n, r, i, a, o, s, c, l);
		} catch (e) {
			if (x(u), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ia(e, t, n, r, i, a, o) {
		var s = S();
		try {
			T(e)(t, n, r, i, a, o);
		} catch (e) {
			if (x(s), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function aa(e, t, n, r, i, a, o, s) {
		var c = S();
		try {
			T(e)(t, n, r, i, a, o, s);
		} catch (e) {
			if (x(c), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function oa(e) {
		var t = S();
		try {
			T(e)();
		} catch (e) {
			if (x(t), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function sa(e, t, n, r, i, a, o, s, c, l, u, d) {
		var f = S();
		try {
			return T(e)(t, n, r, i, a, o, s, c, l, u, d);
		} catch (e) {
			if (x(f), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ca(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function la(e, t) {
		var n = S();
		try {
			return T(e)(t);
		} catch (e) {
			if (x(n), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ua(e, t, n, r, i, a, o, s, c) {
		var l = S();
		try {
			T(e)(t, n, r, i, a, o, s, c);
		} catch (e) {
			if (x(l), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function da(e, t, n, r, i, a, o, s, c, l, u, d) {
		var f = S();
		try {
			T(e)(t, n, r, i, a, o, s, c, l, u, d);
		} catch (e) {
			if (x(f), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function fa(e, t, n, r) {
		var i = S();
		try {
			return T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function pa(e, t, n, r, i, a) {
		var o = S();
		try {
			T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ma(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ha(e, t, n, r, i, a) {
		var o = S();
		try {
			return T(e)(t, n, r, i, a);
		} catch (e) {
			if (x(o), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ga(e, t, n, r) {
		var i = S();
		try {
			return T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function _a(e, t, n, r) {
		var i = S();
		try {
			return T(e)(t, n, r);
		} catch (e) {
			if (x(i), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function va(e, t, n, r, i, a, o, s, c, l, u) {
		var d = S();
		try {
			T(e)(t, n, r, i, a, o, s, c, l, u);
		} catch (e) {
			if (x(d), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ya(e, t, n, r, i, a, o, s, c, l, u, d, f, p, m, h) {
		var ee = S();
		try {
			T(e)(t, n, r, i, a, o, s, c, l, u, d, f, p, m, h);
		} catch (e) {
			if (x(ee), !(e instanceof g)) throw e;
			$(1, 0);
		}
	}
	function ba(e) {
		e = Object.assign({}, e);
		var t = (e) => (t) => e(t) >>> 0;
		return e.rb = t(e.rb), e.kk = t(e.kk), e.yo = ((e) => (t, n) => e(t, n) >>> 0)(e.yo), e._emscripten_stack_alloc = t(e._emscripten_stack_alloc), e.Co = ((e) => () => e() >>> 0)(e.Co), e.Go = t(e.Go), e;
	}
	async function xa() {
		Ke && await Ge(), !m && re();
	}
	var Sa = await y();
	return await xa(), t;
}
//#endregion
//#region src/image-magick.ts
var et = class {
	constructor(e) {
		if (e instanceof URL) {
			if (e.protocol !== "http:" && e.protocol !== "https:") throw new y("Only http/https protocol is supported");
			this.locateFile = () => e.href;
		} else e instanceof WebAssembly.Module ? this.instantiateWasm = (t, n) => {
			n(new WebAssembly.Instance(e, t));
		} : this.wasmBinary = e;
	}
	wasmBinary;
	instantiateWasm;
	locateFile;
}, R = class {
	api;
	constructor() {}
	async _initialize(e, t, n) {
		if (this.api !== void 0) return;
		let r = await e(new et(t));
		this.writeConfigurationFiles(r, n), fe(r, "MAGICK_CONFIGURE_PATH", (e) => {
			fe(r, "/xml", (t) => {
				r._Environment_SetEnv(e, t), r._Environment_Initialize(), this.api = r;
			});
		});
	}
	static get _api() {
		if (!z.api) throw new y("`await initializeImageMagick` should be called to initialize the library");
		return z.api;
	}
	static set _api(e) {
		z.api = e;
	}
	static read(t, n, r, i) {
		return Qe._create((a) => {
			let o = i;
			if (typeof t != "string" && !e(t)) typeof n == "number" && typeof r == "number" && a.read(t, n, r);
			else if (typeof n != "number" && typeof r != "number") {
				o = r;
				let e;
				n instanceof N ? e = n : typeof n == "string" ? (e = new N(), e.format = n) : o = n, a.read(t, e);
			}
			return o(a);
		});
	}
	static readCollection(e, t, n) {
		return F.use((r) => {
			let i = n, a;
			return t instanceof N ? a = t : typeof t == "string" ? (a = new N(), a.format = t) : i = t, r.read(e, a), i(r);
		});
	}
	static readFromCanvas(e, t, n) {
		return Qe._create((r) => (r.readFromCanvas(e, n), t(r)));
	}
	writeConfigurationFiles(e, t) {
		e.FS.analyzePath("/xml").exists || e.FS.mkdir("/xml");
		for (let n of t.all()) {
			let t = e.FS.open(`/xml/${n.fileName}`, "w"), r = new TextEncoder().encode(n.data);
			e.FS.write(t, r, 0, r.length), e.FS.close(t);
		}
	}
}, z = new R();
async function B(e, t) {
	await z._initialize(L, e, t ?? r.default);
}
async function V(e, t) {
	await z._initialize($e, e, t ?? r.default);
}
//#endregion
//#region src/events/progress-event.ts
var tt = class {
	constructor(e, t, n) {
		this.origin = e, this.progress = new I((t + 1) / (n * 100));
	}
	origin;
	progress;
	cancel = !1;
}, nt = class e {
	static _logDelegate = 0;
	static _onLog;
	static _progressDelegate = 0;
	static _images = {};
	static setLogDelegate(t) {
		e._logDelegate === 0 && t !== void 0 && (e._logDelegate = R._api.addFunction(e.logDelegate, "vii")), R._api._Magick_SetLogDelegate(t === void 0 ? 0 : e._logDelegate), e._onLog = t;
	}
	static setProgressDelegate(t) {
		e._progressDelegate === 0 && (this._progressDelegate = R._api.addFunction(e.progressDelegate, "iijji")), this._images[t._instance.toString()] = t, R._api._MagickImage_SetClientData(t._instance, t._instance), R._api._MagickImage_SetProgressDelegate(t._instance, T(e._progressDelegate));
	}
	static removeProgressDelegate(t) {
		R._api._MagickImage_SetClientData(t._instance, R._api._NullPointer), R._api._MagickImage_SetProgressDelegate(t._instance, R._api._NullPointer), delete e._images[t._instance.toString()];
	}
	static logDelegate(t, n) {
		if (e._onLog === void 0) return;
		let r = b(T(n), "");
		e._onLog(new p(t, r));
	}
	static progressDelegate(t, n, r, i) {
		let a = e._images[i];
		if (a === void 0 || a.onProgress === void 0) return 1;
		let o = Number(n), s = Number(r), c = new tt(b(T(t)), o, s);
		return a.onProgress(c), +!c.cancel;
	}
}, rt = class e {
	static _allFormats;
	constructor(e, t, n, r, i, a, o, s) {
		this.format = e, this.description = t, this.mimeType = n, this.moduleFormat = r, this.supportsMultipleFrames = i, this.supportsReading = a, this.supportsWriting = o, this.version = s;
	}
	description;
	format;
	mimeType;
	moduleFormat;
	supportsMultipleFrames;
	supportsReading;
	supportsWriting;
	version;
	static get all() {
		return e._allFormats === void 0 && (e._allFormats = e.loadFormats()), e._allFormats;
	}
	static loadFormats() {
		return k.usePointer((t) => be.use((n) => {
			let r = R._api._MagickFormatInfo_CreateList(n.ptr, t), i = n.value;
			try {
				let n = Array(Number(i)), a = Object.values(Ae);
				for (let o = 0; o < i; o++) {
					let i = R._api._MagickFormatInfo_GetInfo(r, T(o), t), s = b(R._api._MagickFormatInfo_Format_Get(i)), c = e.convertFormat(s, a), l = b(R._api._MagickFormatInfo_Description_Get(i), ""), u = b(R._api._MagickFormatInfo_MimeType_Get(i)), d = b(R._api._MagickFormatInfo_Module_Get(i)), f = e.convertFormat(d, a), p = R._api._MagickFormatInfo_SupportsMultipleFrames_Get(i) == 1, m = R._api._MagickFormatInfo_SupportsReading_Get(i) == 1, h = R._api._MagickFormatInfo_SupportsWriting_Get(i) == 1, g = b(R._api._MagickFormatInfo_Version_Get(i));
					n[o] = new e(c, l, u, f, p, m, h, g);
				}
				return n;
			} finally {
				R._api._MagickFormatInfo_DisposeList(r, i);
			}
		}));
	}
	static convertFormat(e, t) {
		return e === null ? Ae.Unknown : t.includes(e) ? e : Ae.Unknown;
	}
}, H = {
	None: 0,
	Accelerate: 1,
	Annotate: 2,
	Blob: 4,
	Cache: 8,
	Coder: 16,
	Configure: 32,
	Deprecate: 64,
	Draw: 128,
	Exception: 256,
	Image: 512,
	Locale: 1024,
	Module: 2048,
	Pixel: 4096,
	Policy: 8192,
	Resource: 16384,
	Trace: 32768,
	Transform: 65536,
	User: 131072,
	Wand: 262144,
	Detailed: 2147450879,
	get All() {
		return this.Detailed | this.Trace;
	}
}, it = class e {
	static get delegates() {
		return b(R._api._Magick_Delegates_Get(), "Unknown");
	}
	static get features() {
		return b(R._api._Magick_Features_Get(), " ").slice(0, -1);
	}
	static get imageMagickVersion() {
		return b(R._api._Magick_ImageMagickVersion_Get(), "Unknown");
	}
	static get supportedFormats() {
		return rt.all;
	}
	static onLog;
	static addFont(e, t) {
		let n = R._api.FS;
		n.analyzePath("/fonts").exists || n.mkdir("/fonts");
		let r = n.open(`/fonts/${e}`, "w");
		n.write(r, t, 0, t.length), n.close(r);
	}
	static resetRandomSeed = () => R._api._Magick_ResetRandomSeed();
	static setRandomSeed = (e) => R._api._Magick_SetRandomSeed(e);
	static setLogEvents(t) {
		let n = t == H.None ? void 0 : e.logDelegate;
		nt.setLogDelegate(n), C(e.getEventTypeString(t), (e) => R._api._Magick_SetLogEvents(e));
	}
	static _getFontFileName(e) {
		let t = `/fonts/${e}`;
		if (!R._api.FS.analyzePath(t).exists) throw `Unable to find a font with the name '${e}', register it with the addFont method of the Magick class.`;
		return t;
	}
	static getEventTypeString(e) {
		if (e == H.All) return "All,Trace";
		if (e == H.Detailed) return "All";
		switch (e) {
			case H.Accelerate: return "Accelerate";
			case H.Annotate: return "Annotate";
			case H.Blob: return "Blob";
			case H.Cache: return "Cache";
			case H.Coder: return "Coder";
			case H.Configure: return "Configure";
			case H.Deprecate: return "Deprecate";
			case H.Draw: return "Draw";
			case H.Exception: return "Exception";
			case H.Image: return "Image";
			case H.Locale: return "Locale";
			case H.Module: return "Module";
			case H.Pixel: return "Pixel";
			case H.Policy: return "Policy";
			case H.Resource: return "Resource";
			case H.Trace: return "Trace";
			case H.Transform: return "Transform";
			case H.User: return "User";
			case H.Wand: return "Wand";
			case H.None:
			default: return "None";
		}
	}
	static logDelegate(t) {
		e.onLog !== void 0 && e.onLog(t);
	}
}, at = class {
	_font;
	constructor(e) {
		this._font = e;
	}
	get font() {
		return this._font;
	}
	draw(e) {
		let t = it._getFontFileName(this._font);
		e.font(t);
	}
}, U = class {
	_gravity;
	constructor(e) {
		this._gravity = e;
	}
	get gravity() {
		return this._gravity;
	}
	draw(e) {
		e.gravity(this._gravity);
	}
}, ot = class {
	_startX;
	_startY;
	_endX;
	_endY;
	constructor(e, t, n, r) {
		this._startX = e, this._startY = t, this._endX = n, this._endY = r;
	}
	get startX() {
		return this._startX;
	}
	get startY() {
		return this._startY;
	}
	get endX() {
		return this._endX;
	}
	get endY() {
		return this._endY;
	}
	draw(e) {
		e.line(this._startX, this._startY, this._endX, this._endY);
	}
}, st = class {
	_paths = [];
	constructor(e) {
		this._paths = e;
	}
	draw(e) {
		e.pathStart();
		for (let t of this._paths) t.draw(e);
		e.pathFinish();
	}
}, ct = class {
	_x;
	_y;
	constructor(e, t) {
		this._x = e, this._y = t;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	draw(e) {
		e.point(this._x, this._y);
	}
}, lt = class {
	_upperLeftX;
	_upperLeftY;
	_lowerRightX;
	_lowerRightY;
	constructor(e, t, n, r) {
		this._upperLeftX = e, this._upperLeftY = t, this._lowerRightX = n, this._lowerRightY = r;
	}
	get upperLeftX() {
		return this._upperLeftX;
	}
	get upperLeftY() {
		return this._upperLeftY;
	}
	get lowerRightX() {
		return this._lowerRightX;
	}
	get lowerRightY() {
		return this._lowerRightY;
	}
	draw(e) {
		e.rectangle(this._upperLeftX, this._upperLeftY, this._lowerRightX, this._lowerRightY);
	}
}, ut = class {
	_upperLeftX;
	_upperLeftY;
	_lowerRightX;
	_lowerRightY;
	_cornerWidth;
	_cornerHeight;
	constructor(e, t, n, r, i, a) {
		this._upperLeftX = e, this._upperLeftY = t, this._lowerRightX = n, this._lowerRightY = r, this._cornerWidth = i, this._cornerHeight = a;
	}
	get upperLeftX() {
		return this._upperLeftX;
	}
	get upperLeftY() {
		return this._upperLeftY;
	}
	get lowerRightX() {
		return this._lowerRightX;
	}
	get lowerRightY() {
		return this._lowerRightY;
	}
	get cornerWidth() {
		return this._cornerWidth;
	}
	get cornerHeight() {
		return this._cornerHeight;
	}
	draw(e) {
		e.roundRectangle(this._upperLeftX, this._upperLeftY, this._lowerRightX, this._lowerRightY, this._cornerWidth, this._cornerHeight);
	}
}, dt = class {
	_color;
	constructor(e) {
		this._color = e;
	}
	get color() {
		return this._color;
	}
	draw(e) {
		e.strokeColor(this._color);
	}
}, ft = class {
	_dash = [];
	constructor(e) {
		this._dash = [...e];
	}
	get dash() {
		return this._dash;
	}
	draw(e) {
		e.strokeDashArray(this._dash);
	}
}, pt = class {
	_offset;
	constructor(e) {
		this._offset = e;
	}
	get offset() {
		return this._offset;
	}
	draw(e) {
		e.strokeDashOffset(this._offset);
	}
}, mt = class {
	_width;
	constructor(e) {
		this._width = e;
	}
	get width() {
		return this._width;
	}
	draw(e) {
		e.strokeWidth(this._width);
	}
}, ht = class {
	_alignment;
	constructor(e) {
		this._alignment = e;
	}
	get alignment() {
		return this._alignment;
	}
	draw(e) {
		e.textAlignment(this._alignment);
	}
}, gt = class e {
	_isEnabled;
	constructor(e) {
		this._isEnabled = e;
	}
	static get disabled() {
		return new e(!1);
	}
	static get enabled() {
		return new e(!0);
	}
	get isEnabled() {
		return this._isEnabled;
	}
	draw(e) {
		e.textAntialias(this._isEnabled);
	}
}, _t = class {
	_decoration;
	constructor(e) {
		this._decoration = e;
	}
	get decoration() {
		return this._decoration;
	}
	draw(e) {
		e.textDecoration(this._decoration);
	}
}, vt = class {
	_spacing;
	constructor(e) {
		this._spacing = e;
	}
	get spacing() {
		return this._spacing;
	}
	draw(e) {
		e.textInterlineSpacing(this._spacing);
	}
}, yt = class {
	_spacing;
	constructor(e) {
		this._spacing = e;
	}
	get spacing() {
		return this._spacing;
	}
	draw(e) {
		e.textInterwordspacing(this._spacing);
	}
}, bt = class {
	_kerning;
	constructor(e) {
		this._kerning = e;
	}
	get kerning() {
		return this._kerning;
	}
	draw(e) {
		e.textKerning(this._kerning);
	}
}, xt = class {
	_color;
	constructor(e) {
		this._color = e;
	}
	get color() {
		return this._color;
	}
	draw(e) {
		e.textUnderColor(this._color);
	}
}, St = class {
	_x;
	_y;
	_value;
	constructor(e, t, n) {
		this._x = e, this._y = t, this._value = n;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	get value() {
		return this._value;
	}
	draw(e) {
		e.text(this._x, this._y, this._value);
	}
}, Ct = class {
	static get None() {
		return new w(0, 0, 0, 0);
	}
	static get Transparent() {
		return new w(0, 0, 0, 0);
	}
	static get AliceBlue() {
		return new w(240, 248, 255, 255);
	}
	static get AntiqueWhite() {
		return new w(250, 235, 215, 255);
	}
	static get Aqua() {
		return new w(0, 255, 255, 255);
	}
	static get Aquamarine() {
		return new w(127, 255, 212, 255);
	}
	static get Azure() {
		return new w(240, 255, 255, 255);
	}
	static get Beige() {
		return new w(245, 245, 220, 255);
	}
	static get Bisque() {
		return new w(255, 228, 196, 255);
	}
	static get Black() {
		return new w(0, 0, 0, 255);
	}
	static get BlanchedAlmond() {
		return new w(255, 235, 205, 255);
	}
	static get Blue() {
		return new w(0, 0, 255, 255);
	}
	static get BlueViolet() {
		return new w(138, 43, 226, 255);
	}
	static get Brown() {
		return new w(165, 42, 42, 255);
	}
	static get BurlyWood() {
		return new w(222, 184, 135, 255);
	}
	static get CadetBlue() {
		return new w(95, 158, 160, 255);
	}
	static get Chartreuse() {
		return new w(127, 255, 0, 255);
	}
	static get Chocolate() {
		return new w(210, 105, 30, 255);
	}
	static get Coral() {
		return new w(255, 127, 80, 255);
	}
	static get CornflowerBlue() {
		return new w(100, 149, 237, 255);
	}
	static get Cornsilk() {
		return new w(255, 248, 220, 255);
	}
	static get Crimson() {
		return new w(220, 20, 60, 255);
	}
	static get Cyan() {
		return new w(0, 255, 255, 255);
	}
	static get DarkBlue() {
		return new w(0, 0, 139, 255);
	}
	static get DarkCyan() {
		return new w(0, 139, 139, 255);
	}
	static get DarkGoldenrod() {
		return new w(184, 134, 11, 255);
	}
	static get DarkGray() {
		return new w(169, 169, 169, 255);
	}
	static get DarkGreen() {
		return new w(0, 100, 0, 255);
	}
	static get DarkKhaki() {
		return new w(189, 183, 107, 255);
	}
	static get DarkMagenta() {
		return new w(139, 0, 139, 255);
	}
	static get DarkOliveGreen() {
		return new w(85, 107, 47, 255);
	}
	static get DarkOrange() {
		return new w(255, 140, 0, 255);
	}
	static get DarkOrchid() {
		return new w(153, 50, 204, 255);
	}
	static get DarkRed() {
		return new w(139, 0, 0, 255);
	}
	static get DarkSalmon() {
		return new w(233, 150, 122, 255);
	}
	static get DarkSeaGreen() {
		return new w(143, 188, 143, 255);
	}
	static get DarkSlateBlue() {
		return new w(72, 61, 139, 255);
	}
	static get DarkSlateGray() {
		return new w(47, 79, 79, 255);
	}
	static get DarkTurquoise() {
		return new w(0, 206, 209, 255);
	}
	static get DarkViolet() {
		return new w(148, 0, 211, 255);
	}
	static get DeepPink() {
		return new w(255, 20, 147, 255);
	}
	static get DeepSkyBlue() {
		return new w(0, 191, 255, 255);
	}
	static get DimGray() {
		return new w(105, 105, 105, 255);
	}
	static get DodgerBlue() {
		return new w(30, 144, 255, 255);
	}
	static get Firebrick() {
		return new w(178, 34, 34, 255);
	}
	static get FloralWhite() {
		return new w(255, 250, 240, 255);
	}
	static get ForestGreen() {
		return new w(34, 139, 34, 255);
	}
	static get Fuchsia() {
		return new w(255, 0, 255, 255);
	}
	static get Gainsboro() {
		return new w(220, 220, 220, 255);
	}
	static get GhostWhite() {
		return new w(248, 248, 255, 255);
	}
	static get Gold() {
		return new w(255, 215, 0, 255);
	}
	static get Goldenrod() {
		return new w(218, 165, 32, 255);
	}
	static get Gray() {
		return new w(128, 128, 128, 255);
	}
	static get Green() {
		return new w(0, 128, 0, 255);
	}
	static get GreenYellow() {
		return new w(173, 255, 47, 255);
	}
	static get Honeydew() {
		return new w(240, 255, 240, 255);
	}
	static get HotPink() {
		return new w(255, 105, 180, 255);
	}
	static get IndianRed() {
		return new w(205, 92, 92, 255);
	}
	static get Indigo() {
		return new w(75, 0, 130, 255);
	}
	static get Ivory() {
		return new w(255, 255, 240, 255);
	}
	static get Khaki() {
		return new w(240, 230, 140, 255);
	}
	static get Lavender() {
		return new w(230, 230, 250, 255);
	}
	static get LavenderBlush() {
		return new w(255, 240, 245, 255);
	}
	static get LawnGreen() {
		return new w(124, 252, 0, 255);
	}
	static get LemonChiffon() {
		return new w(255, 250, 205, 255);
	}
	static get LightBlue() {
		return new w(173, 216, 230, 255);
	}
	static get LightCoral() {
		return new w(240, 128, 128, 255);
	}
	static get LightCyan() {
		return new w(224, 255, 255, 255);
	}
	static get LightGoldenrodYellow() {
		return new w(250, 250, 210, 255);
	}
	static get LightGreen() {
		return new w(144, 238, 144, 255);
	}
	static get LightGray() {
		return new w(211, 211, 211, 255);
	}
	static get LightPink() {
		return new w(255, 182, 193, 255);
	}
	static get LightSalmon() {
		return new w(255, 160, 122, 255);
	}
	static get LightSeaGreen() {
		return new w(32, 178, 170, 255);
	}
	static get LightSkyBlue() {
		return new w(135, 206, 250, 255);
	}
	static get LightSlateGray() {
		return new w(119, 136, 153, 255);
	}
	static get LightSteelBlue() {
		return new w(176, 196, 222, 255);
	}
	static get LightYellow() {
		return new w(255, 255, 224, 255);
	}
	static get Lime() {
		return new w(0, 255, 0, 255);
	}
	static get LimeGreen() {
		return new w(50, 205, 50, 255);
	}
	static get Linen() {
		return new w(250, 240, 230, 255);
	}
	static get Magenta() {
		return new w(255, 0, 255, 255);
	}
	static get Maroon() {
		return new w(128, 0, 0, 255);
	}
	static get MediumAquamarine() {
		return new w(102, 205, 170, 255);
	}
	static get MediumBlue() {
		return new w(0, 0, 205, 255);
	}
	static get MediumOrchid() {
		return new w(186, 85, 211, 255);
	}
	static get MediumPurple() {
		return new w(147, 112, 219, 255);
	}
	static get MediumSeaGreen() {
		return new w(60, 179, 113, 255);
	}
	static get MediumSlateBlue() {
		return new w(123, 104, 238, 255);
	}
	static get MediumSpringGreen() {
		return new w(0, 250, 154, 255);
	}
	static get MediumTurquoise() {
		return new w(72, 209, 204, 255);
	}
	static get MediumVioletRed() {
		return new w(199, 21, 133, 255);
	}
	static get MidnightBlue() {
		return new w(25, 25, 112, 255);
	}
	static get MintCream() {
		return new w(245, 255, 250, 255);
	}
	static get MistyRose() {
		return new w(255, 228, 225, 255);
	}
	static get Moccasin() {
		return new w(255, 228, 181, 255);
	}
	static get NavajoWhite() {
		return new w(255, 222, 173, 255);
	}
	static get Navy() {
		return new w(0, 0, 128, 255);
	}
	static get OldLace() {
		return new w(253, 245, 230, 255);
	}
	static get Olive() {
		return new w(128, 128, 0, 255);
	}
	static get OliveDrab() {
		return new w(107, 142, 35, 255);
	}
	static get Orange() {
		return new w(255, 165, 0, 255);
	}
	static get OrangeRed() {
		return new w(255, 69, 0, 255);
	}
	static get Orchid() {
		return new w(218, 112, 214, 255);
	}
	static get PaleGoldenrod() {
		return new w(238, 232, 170, 255);
	}
	static get PaleGreen() {
		return new w(152, 251, 152, 255);
	}
	static get PaleTurquoise() {
		return new w(175, 238, 238, 255);
	}
	static get PaleVioletRed() {
		return new w(219, 112, 147, 255);
	}
	static get PapayaWhip() {
		return new w(255, 239, 213, 255);
	}
	static get PeachPuff() {
		return new w(255, 218, 185, 255);
	}
	static get Peru() {
		return new w(205, 133, 63, 255);
	}
	static get Pink() {
		return new w(255, 192, 203, 255);
	}
	static get Plum() {
		return new w(221, 160, 221, 255);
	}
	static get PowderBlue() {
		return new w(176, 224, 230, 255);
	}
	static get Purple() {
		return new w(128, 0, 128, 255);
	}
	static get RebeccaPurple() {
		return new w(102, 51, 153, 255);
	}
	static get Red() {
		return new w(255, 0, 0, 255);
	}
	static get RosyBrown() {
		return new w(188, 143, 143, 255);
	}
	static get RoyalBlue() {
		return new w(65, 105, 225, 255);
	}
	static get SaddleBrown() {
		return new w(139, 69, 19, 255);
	}
	static get Salmon() {
		return new w(250, 128, 114, 255);
	}
	static get SandyBrown() {
		return new w(244, 164, 96, 255);
	}
	static get SeaGreen() {
		return new w(46, 139, 87, 255);
	}
	static get SeaShell() {
		return new w(255, 245, 238, 255);
	}
	static get Sienna() {
		return new w(160, 82, 45, 255);
	}
	static get Silver() {
		return new w(192, 192, 192, 255);
	}
	static get SkyBlue() {
		return new w(135, 206, 235, 255);
	}
	static get SlateBlue() {
		return new w(106, 90, 205, 255);
	}
	static get SlateGray() {
		return new w(112, 128, 144, 255);
	}
	static get Snow() {
		return new w(255, 250, 250, 255);
	}
	static get SpringGreen() {
		return new w(0, 255, 127, 255);
	}
	static get SteelBlue() {
		return new w(70, 130, 180, 255);
	}
	static get Tan() {
		return new w(210, 180, 140, 255);
	}
	static get Teal() {
		return new w(0, 128, 128, 255);
	}
	static get Thistle() {
		return new w(216, 191, 216, 255);
	}
	static get Tomato() {
		return new w(255, 99, 71, 255);
	}
	static get Turquoise() {
		return new w(64, 224, 208, 255);
	}
	static get Violet() {
		return new w(238, 130, 238, 255);
	}
	static get Wheat() {
		return new w(245, 222, 179, 255);
	}
	static get White() {
		return new w(255, 255, 255, 255);
	}
	static get WhiteSmoke() {
		return new w(245, 245, 245, 255);
	}
	static get Yellow() {
		return new w(255, 255, 0, 255);
	}
	static get YellowGreen() {
		return new w(154, 205, 50, 255);
	}
}, wt = class {
	_x;
	_y;
	constructor(e, t) {
		this._x = e, this._y = t;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	draw(e) {
		e.pathLineToAbs(this._x, this._y);
	}
}, Tt = class {
	_x;
	_y;
	constructor(e, t) {
		this._x = e, this._y = t;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	draw(e) {
		e.pathLineToRel(this._x, this._y);
	}
}, Et = class {
	_x;
	_y;
	constructor(e, t) {
		this._x = e, this._y = t;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	draw(e) {
		e.pathMoveToAbs(this._x, this._y);
	}
}, Dt = class {
	_x;
	_y;
	constructor(e, t) {
		this._x = e, this._y = t;
	}
	get x() {
		return this._x;
	}
	get y() {
		return this._y;
	}
	draw(e) {
		e.pathMoveToRel(this._x, this._y);
	}
}, Ot = class {
	_drawables;
	_paths = [];
	constructor(e) {
		this._drawables = e;
	}
	drawables() {
		return this._drawables === void 0 ? new kt().path(this._paths) : this._drawables.path(this._paths);
	}
	lineToAbs(e, t) {
		return this._paths.push(new wt(e, t)), this;
	}
	lineToRel(e, t) {
		return this._paths.push(new Tt(e, t)), this;
	}
	moveToAbs(e, t) {
		return this._paths.push(new Et(e, t)), this;
	}
	moveToRel(e, t) {
		return this._paths.push(new Dt(e, t)), this;
	}
}, kt = class {
	_drawables = [];
	affine(e = 1, t = 1, n = 0, r = 0, i = 0, a = 0) {
		return this._drawables.push(new o(e, t, n, r, i, a)), this;
	}
	color(e, t, n) {
		return this._drawables.push(new c(e, t, n)), this;
	}
	borderColor(e) {
		return this._drawables.push(new s(e)), this;
	}
	disableTextAntiAlias() {
		return this._drawables.push(gt.disabled), this;
	}
	draw(e) {
		return e.draw(this._drawables), this;
	}
	enableTextAntiAlias() {
		return this._drawables.push(gt.enabled), this;
	}
	fillColor(e) {
		return this._drawables.push(new l(e)), this;
	}
	fillOpacity(e) {
		return this._drawables.push(new u(e)), this;
	}
	fillRule(e) {
		return this._drawables.push(new d(e)), this;
	}
	font(e) {
		return this._drawables.push(new at(e)), this;
	}
	fontPointSize(e) {
		return this._drawables.push(new f(e)), this;
	}
	fontTypeMetrics(e, t = !1) {
		return Qe._create((n) => (n.read(Ct.Transparent, 1, 1), Ee._use(n, (n) => (n.draw(this._drawables), n.fontTypeMetrics(e, t)))));
	}
	gravity(e) {
		return this._drawables.push(new U(e)), this;
	}
	line(e, t, n, r) {
		return this._drawables.push(new ot(e, t, n, r)), this;
	}
	path(e) {
		return this._drawables.push(new st(e)), this;
	}
	paths() {
		return new Ot(this);
	}
	point(e, t) {
		return this._drawables.push(new ct(e, t)), this;
	}
	rectangle(e, t, n, r) {
		return this._drawables.push(new lt(e, t, n, r)), this;
	}
	roundRectangle(e, t, n, r, i, a) {
		return this._drawables.push(new ut(e, t, n, r, i, a)), this;
	}
	strokeColor(e) {
		return this._drawables.push(new dt(e)), this;
	}
	strokeDashArray(e) {
		return this._drawables.push(new ft(e)), this;
	}
	strokeDashOffset(e) {
		return this._drawables.push(new pt(e)), this;
	}
	strokeWidth(e) {
		return this._drawables.push(new mt(e)), this;
	}
	text(e, t, n) {
		return this._drawables.push(new St(e, t, n)), this;
	}
	textAlignment(e) {
		return this._drawables.push(new ht(e)), this;
	}
	textDecoration(e) {
		return this._drawables.push(new _t(e)), this;
	}
	textInterlineSpacing(e) {
		return this._drawables.push(new vt(e)), this;
	}
	textInterwordSpacing(e) {
		return this._drawables.push(new yt(e)), this;
	}
	textKerning(e) {
		return this._drawables.push(new bt(e)), this;
	}
	textUnderColor(e) {
		return this._drawables.push(new xt(e)), this;
	}
}, At = {
	Undefined: 0,
	Kapur: 1,
	OTSU: 2,
	Triangle: 3
}, jt = {
	Undefined: 0,
	Direct: 1,
	Pseudo: 2
}, W = {
	Undefined: 0,
	Bilevel: 1,
	Grayscale: 2,
	GrayscaleAlpha: 3,
	Palette: 4,
	PaletteAlpha: 5,
	TrueColor: 6,
	TrueColorAlpha: 7,
	ColorSeparation: 8,
	ColorSeparationAlpha: 9,
	Optimize: 10,
	PaletteBilevelAlpha: 11
}, G = {
	Undefined: 0,
	Add: 1,
	Conjugate: 2,
	Divide: 3,
	MagnitudePhase: 4,
	Multiply: 5,
	RealImaginary: 6,
	Subtract: 7
}, K = {
	Undefined: 0,
	B44A: 1,
	B44: 2,
	BZip: 3,
	DXT1: 4,
	DXT3: 5,
	DXT5: 6,
	Fax: 7,
	Group4: 8,
	JBIG1: 9,
	JBIG2: 10,
	JPEG2000: 11,
	JPEG: 12,
	LosslessJPEG: 13,
	LZMA: 14,
	LZW: 15,
	NoCompression: 16,
	Piz: 17,
	Pxr24: 18,
	RLE: 19,
	Zip: 20,
	ZipS: 21,
	Zstd: 22,
	WebP: 23,
	DWAA: 24,
	DWAB: 25,
	BC7: 26,
	BC5: 27
}, q = {
	Undefined: 0,
	Affine: 1,
	AffineProjection: 2,
	ScaleRotateTranslate: 3,
	Perspective: 4,
	PerspectiveProjection: 5,
	BilinearForward: 6,
	BilinearReverse: 7,
	Polynomial: 8,
	Arc: 9,
	Polar: 10,
	DePolar: 11,
	Cylinder2Plane: 12,
	Plane2Cylinder: 13,
	Barrel: 14,
	BarrelInverse: 15,
	Shepards: 16,
	Resize: 17,
	Sentinel: 18,
	RigidAffine: 19
}, Mt = {
	Undefined: 0,
	LSB: 1,
	MSB: 2
}, J = {
	Undefined: 0,
	Absolute: 1,
	Fuzz: 2,
	MeanAbsolute: 3,
	MeanErrorPerPixel: 4,
	MeanSquared: 5,
	NormalizedCrossCorrelation: 6,
	PeakAbsolute: 7,
	PeakSignalToNoiseRatio: 8,
	PerceptualHash: 9,
	RootMeanSquared: 10,
	StructuralSimilarity: 11,
	StructuralDissimilarity: 12,
	PixelDifferenceCount: 13
}, Nt = {
	Undefined: 0,
	Abs: 1,
	Add: 2,
	AddModulus: 3,
	And: 4,
	Cosine: 5,
	Divide: 6,
	Exponential: 7,
	GaussianNoise: 8,
	ImpulseNoise: 9,
	LaplacianNoise: 10,
	LeftShift: 11,
	Log: 12,
	Max: 13,
	Mean: 14,
	Median: 15,
	Min: 16,
	MultiplicativeNoise: 17,
	Multiply: 18,
	Or: 19,
	PoissonNoise: 20,
	Pow: 21,
	RightShift: 22,
	RootMeanSquare: 23,
	Set: 24,
	Sine: 25,
	Subtract: 26,
	Sum: 27,
	ThresholdBlack: 28,
	Threshold: 29,
	ThresholdWhite: 30,
	UniformNoise: 31,
	Xor: 32,
	InverseLog: 33
}, Pt = {
	Undefined: 0,
	Point: 1,
	Box: 2,
	Triangle: 3,
	Hermite: 4,
	Hann: 5,
	Hamming: 6,
	Blackman: 7,
	Gaussian: 8,
	Quadratic: 9,
	Cubic: 10,
	Catrom: 11,
	Mitchell: 12,
	Jinc: 13,
	Sinc: 14,
	SincFast: 15,
	Kaiser: 16,
	Welch: 17,
	Parzen: 18,
	Bohman: 19,
	Bartlett: 20,
	Lagrange: 21,
	Lanczos: 22,
	LanczosSharp: 23,
	Lanczos2: 24,
	Lanczos2Sharp: 25,
	Robidoux: 26,
	RobidouxSharp: 27,
	Cosine: 28,
	Spline: 29,
	LanczosRadius: 30,
	CubicSpline: 31,
	MagicKernelSharp2013: 32,
	MagicKernelSharp2021: 33
}, Ft = {
	Undefined: 0,
	None: 1,
	Background: 2,
	Previous: 3
}, It = {
	Undefined: 0,
	NoInterlace: 1,
	Line: 2,
	Plane: 3,
	Partition: 4,
	Gif: 5,
	Jpeg: 6,
	Png: 7
}, Lt = {
	Undefined: "Undefined",
	Unity: "Unity",
	Gaussian: "Gaussian",
	DoG: "DoG",
	LoG: "LoG",
	Blur: "Blur",
	Comet: "Comet",
	Binomial: "Binomial",
	Laplacian: "Laplacian",
	Sobel: "Sobel",
	FreiChen: "FreiChen",
	Roberts: "Roberts",
	Prewitt: "Prewitt",
	Compass: "Compass",
	Kirsch: "Kirsch",
	Diamond: "Diamond",
	Square: "Square",
	Rectangle: "Rectangle",
	Octagon: "Octagon",
	Disk: "Disk",
	Plus: "Plus",
	Cross: "Cross",
	Ring: "Ring",
	Peaks: "Peaks",
	Edges: "Edges",
	Corners: "Corners",
	Diagonals: "Diagonals",
	LineEnds: "LineEnds",
	LineJunctions: "LineJunctions",
	Ridges: "Ridges",
	ConvexHull: "ConvexHull",
	ThinSE: "ThinSE",
	Skeleton: "Skeleton",
	Chebyshev: "Chebyshev",
	Manhattan: "Manhattan",
	Octagonal: "Octagonal",
	Euclidean: "Euclidean",
	UserDefined: "UserDefined"
}, Rt = {
	Undefined: 0,
	Convolve: 1,
	Correlate: 2,
	Erode: 3,
	Dilate: 4,
	ErodeIntensity: 5,
	DilateIntensity: 6,
	IterativeDistance: 7,
	Open: 8,
	Close: 9,
	OpenIntensity: 10,
	CloseIntensity: 11,
	Smooth: 12,
	EdgeIn: 13,
	EdgeOut: 14,
	Edge: 15,
	TopHat: 16,
	BottomHat: 17,
	HitAndMiss: 18,
	Thinning: 19,
	Thicken: 20,
	Distance: 21,
	Voronoi: 22
}, zt = {
	Undefined: 0,
	Uniform: 1,
	Gaussian: 2,
	MultiplicativeGaussian: 3,
	Impulse: 4,
	Laplacian: 5,
	Poisson: 6,
	Random: 7
}, Bt = {
	Undefined: 0,
	TopLeft: 1,
	TopRight: 2,
	BottomRight: 3,
	BottomLeft: 4,
	LeftTop: 5,
	RightTop: 6,
	RightBottom: 7,
	LeftBottom: 8
}, Vt = {
	Undefined: 0,
	Point: 1,
	Replace: 2,
	Floodfill: 3,
	FillToBorder: 4,
	Reset: 5
}, Ht = {
	Undefined: 0,
	Saturation: 1,
	Perceptual: 2,
	Absolute: 3,
	Relative: 4
}, Ut = {
	Undefined: 0,
	Left: 1,
	Center: 2,
	Right: 3
}, Wt = {
	Undefined: 0,
	None: 1,
	Underline: 2,
	Overline: 3,
	LineThrough: 4
}, Gt = {
	Undefined: 0,
	Background: 1,
	Dither: 2,
	Edge: 3,
	Mirror: 4,
	Random: 5,
	Tile: 6,
	Transparent: 7,
	Mask: 8,
	Black: 9,
	Gray: 10,
	White: 11,
	HorizontalTile: 12,
	VerticalTile: 13,
	HorizontalTileEdge: 14,
	VerticalTileEdge: 15,
	CheckerTile: 16
}, Kt = /* @__PURE__ */ function(e) {
	return e[e.Disabled = -1] = "Disabled", e[e.Linear = 0] = "Linear", e[e.Vng = 1] = "Vng", e[e.Ppg = 2] = "Ppg", e[e.Ahd = 3] = "Ahd", e[e.DCB = 4] = "DCB", e[e.Dht = 11] = "Dht", e[e.ModifiedAhd = 12] = "ModifiedAhd", e;
}({}), qt = /* @__PURE__ */ function(e) {
	return e[e.Raw = 0] = "Raw", e[e.SRGB = 1] = "SRGB", e[e.AdobeRGB = 2] = "AdobeRGB", e[e.WideGamutRGB = 3] = "WideGamutRGB", e[e.KodakProPhotoRGB = 4] = "KodakProPhotoRGB", e[e.XYZ = 5] = "XYZ", e[e.ACES = 6] = "ACES", e;
}({}), Jt = class extends a {
	constructor() {
		super(Ae.Dng);
	}
	disableAutoBrightness;
	interpolationQuality;
	outputColor;
	useAutoWhitebalance;
	useCameraWhitebalance;
	getDefines() {
		let e = [];
		return this.hasValue(this.interpolationQuality) && e.push(this.createDefine("interpolation-quality", this.interpolationQuality)), this.hasValue(this.disableAutoBrightness) && e.push(this.createDefine("no-auto-bright", this.disableAutoBrightness)), this.hasValue(this.outputColor) && e.push(this.createDefine("output-color", this.outputColor)), this.hasValue(this.useCameraWhitebalance) && e.push(this.createDefine("use-camera-wb", this.useCameraWhitebalance)), this.hasValue(this.useAutoWhitebalance) && e.push(this.createDefine("use-auto-wb", this.useAutoWhitebalance)), e;
	}
}, Yt = class e {
	_colorSpace = _.Undefined;
	_compression = K.Undefined;
	_density = new _e(0, 0);
	_format = Ae.Unknown;
	_height = 0;
	_interlace = It.Undefined;
	_orientation = Bt.Undefined;
	_quality = 0;
	_width = 0;
	get colorSpace() {
		return this._colorSpace;
	}
	get compression() {
		return this._compression;
	}
	get density() {
		return this._density;
	}
	get format() {
		return this._format;
	}
	get height() {
		return this._height;
	}
	get interlace() {
		return this._interlace;
	}
	get orientation() {
		return this._orientation;
	}
	get quality() {
		return this._quality;
	}
	get width() {
		return this._width;
	}
	constructor() {}
	read(e, t) {
		Qe._create((n) => {
			n.ping(e, t), this._colorSpace = n.colorSpace, this._compression = n.compression, this._density = n.density, this._format = n.format, this._height = n.height, this._interlace = n.interlace, this._orientation = n.orientation, this._quality = n.quality, this._width = n.width;
		});
	}
	static create(t, n) {
		let r = new e();
		return r.read(t, n), r;
	}
}, Xt = class {
	static get area() {
		return R._api._ResourceLimits_Area_Get();
	}
	static set area(e) {
		R._api._ResourceLimits_Area_Set(e);
	}
	static get disk() {
		return R._api._ResourceLimits_Disk_Get();
	}
	static set disk(e) {
		R._api._ResourceLimits_Disk_Set(e);
	}
	static get height() {
		return R._api._ResourceLimits_Height_Get();
	}
	static set height(e) {
		R._api._ResourceLimits_Height_Set(e);
	}
	static get listLength() {
		return R._api._ResourceLimits_ListLength_Get();
	}
	static set listLength(e) {
		R._api._ResourceLimits_ListLength_Set(e);
	}
	static get maxMemoryRequest() {
		return R._api._ResourceLimits_MaxMemoryRequest_Get();
	}
	static set maxMemoryRequest(e) {
		k.usePointer((t) => {
			R._api._ResourceLimits_MaxMemoryRequest_Set(e, t);
		});
	}
	static get maxProfileSize() {
		return R._api._ResourceLimits_MaxProfileSize_Get();
	}
	static set maxProfileSize(e) {
		k.usePointer((t) => {
			R._api._ResourceLimits_MaxProfileSize_Set(e, t);
		});
	}
	static get memory() {
		return R._api._ResourceLimits_Memory_Get();
	}
	static set memory(e) {
		R._api._ResourceLimits_Memory_Set(e);
	}
	static get time() {
		return R._api._ResourceLimits_Time_Get();
	}
	static set time(e) {
		R._api._ResourceLimits_Time_Set(e);
	}
	static get width() {
		return R._api._ResourceLimits_Width_Get();
	}
	static set width(e) {
		R._api._ResourceLimits_Width_Set(e);
	}
}, Zt = class {
	constructor(e) {
		this.complexOperator = e;
	}
	complexOperator;
	signalToNoiseRatio;
	_setArtifacts(e) {
		this.signalToNoiseRatio !== void 0 && e.setArtifact("complex:snr", this.signalToNoiseRatio);
	}
}, Qt = class {
	constructor(e) {
		this.method = e;
	}
	method;
	bestFit = !1;
	scale;
	viewport;
	_setArtifacts(e) {
		this.scale !== void 0 && e.setArtifact("distort:scale", this.scale.toString()), this.viewport !== void 0 && e.setArtifact("distort:viewport", this.viewport.toString());
	}
}, $t = class extends xe {
	constructor(e) {
		let t = R._api._MontageSettings_Create(), n = R._api._MontageSettings_Dispose;
		super(t, n), e.backgroundColor !== void 0 && e.backgroundColor._use((e) => {
			R._api._MontageSettings_SetBackgroundColor(this._instance, e);
		}), e.borderColor !== void 0 && e.borderColor._use((e) => {
			R._api._MontageSettings_SetBorderColor(this._instance, e);
		}), e.borderWidth !== void 0 && R._api._MontageSettings_SetBorderWidth(this._instance, T(e.borderWidth)), e.fillColor !== void 0 && e.fillColor._use((e) => {
			R._api._MontageSettings_SetFillColor(this._instance, e);
		}), e.font !== void 0 && C(it._getFontFileName(e.font), (e) => {
			R._api._MontageSettings_SetFont(this._instance, e);
		}), e.fontPointsize !== void 0 && R._api._MontageSettings_SetFontPointsize(this._instance, e.fontPointsize), e.frameGeometry !== void 0 && C(e.frameGeometry.toString(), (e) => {
			R._api._MontageSettings_SetFrameGeometry(this._instance, e);
		}), e.geometry !== void 0 && C(e.geometry.toString(), (e) => {
			R._api._MontageSettings_SetGeometry(this._instance, e);
		}), e.gravity !== void 0 && R._api._MontageSettings_SetGravity(this._instance, T(e.gravity)), e.shadow !== void 0 && R._api._MontageSettings_SetShadow(this._instance, +!!e.shadow), e.strokeColor !== void 0 && e.strokeColor._use((e) => {
			R._api._MontageSettings_SetStrokeColor(this._instance, e);
		}), e.textureFileName !== void 0 && C(e.textureFileName, (e) => {
			R._api._MontageSettings_SetTextureFileName(this._instance, e);
		}), e.tileGeometry !== void 0 && C(e.tileGeometry.toString(), (e) => {
			R._api._MontageSettings_SetTileGeometry(this._instance, e);
		}), e.title !== void 0 && C(e.title, (e) => {
			R._api._MontageSettings_SetTitle(this._instance, e);
		});
	}
}, en = class {
	backgroundColor;
	borderColor;
	borderWidth;
	fillColor;
	font;
	fontPointsize;
	frameGeometry;
	geometry;
	gravity;
	label;
	shadow;
	strokeColor;
	textureFileName;
	tileGeometry;
	title;
	transparentColor;
	_use(e) {
		let t = new $t(this);
		return O._disposeAfterExecution(t, e);
	}
}, tn = class {
	constructor(e, t, n) {
		this.method = e, this.kernel = t, n !== void 0 && (this.kernel += `:${n}`);
	}
	channels = g.Composite;
	convolveBias;
	convolveScale;
	iterations = 1;
	kernel;
	method;
}, nn = class {
	constructor(e, t = 0) {
		this.minimum = e, this.maximum = t;
	}
	minimum;
	maximum;
	toString() {
		return this.maximum === 0 ? this.minimum.toString() : `${this.minimum}-${this.maximum}`;
	}
};
//#endregion
export { m as AlphaAction, At as AutoThresholdMethod, He as ChannelPerceptualHash, Je as ChannelStatistics, g as Channels, ee as ChromaticityInfo, jt as ClassType, ae as ColorProfile, _ as ColorSpace, te as ColorSpaceNames, oe as ColorTransformMode, W as ColorType, se as CompareResult, ce as CompareSettings, G as ComplexOperator, Zt as ComplexSettings, le as CompositeOperator, K as CompressionMethod, t as ConfigurationFile, r as ConfigurationFiles, he as ConnectedComponent, ge as ConnectedComponentsSettings, a as DefinesCreator, _e as Density, D as DensityUnit, q as DistortMethod, Qt as DistortSettings, P as DitherMethod, Kt as DngInterpolation, qt as DngOutputColor, Jt as DngReadDefines, o as DrawableAffine, s as DrawableBorderColor, c as DrawableColor, l as DrawableFillColor, u as DrawableFillOpacity, d as DrawableFillRule, at as DrawableFont, f as DrawableFontPointSize, U as DrawableGravity, ot as DrawableLine, st as DrawablePath, ct as DrawablePoint, lt as DrawableRectangle, ut as DrawableRoundRectangle, dt as DrawableStrokeColor, ft as DrawableStrokeDashArray, pt as DrawableStrokeDashOffset, mt as DrawableStrokeWidth, St as DrawableText, ht as DrawableTextAlignment, gt as DrawableTextAntiAlias, _t as DrawableTextDecoration, vt as DrawableTextInterlineSpacing, yt as DrawableTextInterwordSpacing, bt as DrawableTextKerning, xt as DrawableTextUnderColor, kt as Drawables, Ee as DrawingWand, Mt as Endian, J as ErrorMetric, Nt as EvaluateOperator, Ne as FillRule, Pt as FilterType, Ft as GifDisposeMethod, j as Gravity, R as ImageMagick, ie as ImageProfile, It as Interlace, Lt as Kernel, p as LogEvent, H as LogEventTypes, it as Magick, w as MagickColor, Ct as MagickColors, i as MagickDefine, y as MagickError, ke as MagickErrorInfo, ue as MagickErrorSeverity, Ae as MagickFormat, rt as MagickFormatInfo, E as MagickGeometry, Qe as MagickImage, F as MagickImageCollection, Yt as MagickImageInfo, N as MagickReadSettings, Fe as MagickSettings, en as MontageSettings, Rt as MorphologyMethod, tn as MorphologySettings, xe as NativeInstance, zt as NoiseType, Be as OffsetInfo, Bt as Orientation, Vt as PaintMethod, wt as PathLineToAbs, Tt as PathLineToRel, Et as PathMoveToAbs, Dt as PathMoveToRel, Ot as Paths, I as Percentage, Ue as PerceptualHash, h as PixelChannel, We as PixelCollection, Ge as PixelIntensityMethod, Ke as PixelInterpolateMethod, me as Point, qe as PrimaryInfo, tt as ProgressEvent, Le as QuantizeSettings, de as Quantum, Ht as RenderingIntent, Xt as ResourceLimits, Ye as Statistics, Ut as TextAlignment, Wt as TextDecoration, nn as Threshold, Se as TypeMetric, Gt as VirtualPixelMethod, Ze as WarningEvent, De as _getGravityEdges, Oe as _getGravityName, e as _isByteArray, B as initializeImageMagick, V as initializeImageMagickx64 };
