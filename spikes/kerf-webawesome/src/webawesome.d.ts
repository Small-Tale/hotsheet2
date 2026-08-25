import type { AttrLike, KerfBaseAttrs } from 'kerfjs/jsx-runtime';

type WaBase = KerfBaseAttrs & { slot?: AttrLike<string> };

declare module 'kerfjs/jsx-runtime' {
  namespace JSX {
    interface IntrinsicElements {
      'wa-button': WaBase & {
        type?: AttrLike<'button' | 'submit' | 'reset'>;
        variant?: AttrLike<'brand' | 'neutral' | 'success' | 'warning' | 'danger'>;
        disabled?: AttrLike<boolean>;
      };
      'wa-input': WaBase & {
        name?: AttrLike<string>;
        label?: AttrLike<string>;
        hint?: AttrLike<string>;
        value?: AttrLike<string>;
        required?: AttrLike<boolean>;
      };
      'wa-dialog': WaBase & {
        label?: AttrLike<string>;
        open?: AttrLike<boolean>;
        'light-dismiss'?: AttrLike<boolean>;
      };
    }
  }
}
