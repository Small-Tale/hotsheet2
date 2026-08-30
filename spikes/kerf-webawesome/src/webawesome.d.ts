import type { AttrLike, KerfBaseAttrs } from 'kerfjs/jsx-runtime';

type WaBase = KerfBaseAttrs & { slot?: AttrLike };

declare module 'kerfjs/jsx-runtime' {
  namespace JSX {
    interface IntrinsicElements {
      'wa-button': WaBase & {
        type?: AttrLike<'button' | 'submit' | 'reset'>;
        variant?: AttrLike<'brand' | 'neutral' | 'success' | 'warning' | 'danger'>;
        disabled?: AttrLike<boolean>;
      };
      'wa-input': WaBase & {
        name?: AttrLike;
        label?: AttrLike;
        hint?: AttrLike;
        value?: AttrLike;
        required?: AttrLike<boolean>;
      };
      'wa-dialog': WaBase & {
        label?: AttrLike;
        open?: AttrLike<boolean>;
        'light-dismiss'?: AttrLike<boolean>;
      };
    }
  }
}
