import type { ButtonHTMLAttributes, Ref } from 'react';
import styles from './Button.module.css';

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'type'> & {
  variant?: 'primary' | 'secondary';
  /** Defaults to "button" so a button never submits a form by accident. */
  type?: 'button' | 'submit';
  ref?: Ref<HTMLButtonElement>;
};

export function Button({
  variant = 'secondary',
  type = 'button',
  className,
  ...rest
}: ButtonProps) {
  const classes = [styles.button, styles[variant], className].filter(Boolean).join(' ');
  return <button type={type} className={classes} {...rest} />;
}
