import type {AttrLike,KerfBaseAttrs} from 'kerfjs/jsx-runtime';
type WaBase=KerfBaseAttrs&{slot?:AttrLike<string>;name?:AttrLike<string>;value?:AttrLike<string>;label?:AttrLike<string>;disabled?:AttrLike<boolean>;checked?:AttrLike<boolean>;size?:AttrLike<string>;variant?:AttrLike<string>;title?:AttrLike<string>;type?:AttrLike<string>};
declare module 'kerfjs/jsx-runtime' { namespace JSX { interface IntrinsicElements {'wa-button':WaBase;'wa-input':WaBase;'wa-select':WaBase;'wa-option':WaBase;'wa-checkbox':WaBase;'wa-dialog':WaBase} } }
