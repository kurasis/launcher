import type { ButtonHTMLAttributes, Ref } from 'react';
import styles from './Button.module.css';

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'type'> & {
  /** "ghost" is a quiet action drawn as text, with no fill or border until hovered;
   * "danger" one that deletes or stops, drawn in the danger color, never filled. */
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  /** "large" is for the screen's main call to action, such as Play; "small" for the
   * actions of a row in a list. */
  size?: 'small' | 'normal' | 'large';
  /** Defaults to "button" so a button never submits a form by accident. */
  type?: 'button' | 'submit';
  ref?: Ref<HTMLButtonElement>;
};

export function Button({
  variant = 'secondary',
  size = 'normal',
  type = 'button',
  className,
  ...rest
}: ButtonProps) {
  const classes = [styles.button, styles[variant], size !== 'normal' && styles[size], className]
    .filter(Boolean)
    .join(' ');
  return <button type={type} className={classes} {...rest} />;
}
