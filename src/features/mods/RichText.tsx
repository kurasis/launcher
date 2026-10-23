import type { ReactNode } from 'react';
import type { DocNode } from '../../lib/contracts';
import { ourPicture } from './helpers';
import styles from './Mods.module.css';

interface Props {
  nodes: DocNode[];
  /** A link was pressed; the caller asks before opening it. */
  onLink: (url: string) => void;
}

function children(nodes: DocNode[] | undefined, onLink: Props['onLink']): ReactNode[] {
  return (nodes ?? []).map((node, index) => draw(node, index, onLink));
}

/** Whether a node shows anything: text that is not blank, a picture, a rule. */
function shows(node: DocNode): boolean {
  if (node.k === 'text' || node.k === 'code') return (node.text ?? '').trim() !== '';
  if (node.k === 'img' || node.k === 'hr') return true;
  return (node.c ?? []).some(shows);
}

/** One node as an element of our own; anything unknown is plain content. */
function draw(node: DocNode, key: number, onLink: Props['onLink']): ReactNode {
  const inner = children(node.c, onLink);
  switch (node.k) {
    case 'text':
      return node.text ?? '';
    case 'p':
      // Authors leave empty lines and headings for room; our spacing is enough.
      return shows(node) ? <p key={key}>{inner}</p> : null;
    case 'block':
      return <div key={key}>{inner}</div>;
    // The project's name and its panels' titles are above, so a description's levels
    // start lower.
    case 'h1':
      return shows(node) ? <h4 key={key}>{inner}</h4> : null;
    case 'h2':
      return shows(node) ? <h5 key={key}>{inner}</h5> : null;
    case 'h3':
    case 'h4':
    case 'h5':
    case 'h6':
      return shows(node) ? <h6 key={key}>{inner}</h6> : null;
    case 'quote':
      return <blockquote key={key}>{inner}</blockquote>;
    case 'pre':
      return (
        <pre key={key} dir="ltr">
          {inner}
        </pre>
      );
    case 'ul':
      return <ul key={key}>{inner}</ul>;
    case 'ol':
      return <ol key={key}>{inner}</ol>;
    case 'li':
      return <li key={key}>{inner}</li>;
    case 'table':
      return (
        <div key={key} className={styles.tableWrap}>
          <table>
            <tbody>{inner}</tbody>
          </table>
        </div>
      );
    case 'tr':
      return <tr key={key}>{inner}</tr>;
    case 'th':
      return <th key={key}>{inner}</th>;
    case 'td':
      return <td key={key}>{inner}</td>;
    case 'em':
      return <em key={key}>{inner}</em>;
    case 'strong':
      return <strong key={key}>{inner}</strong>;
    case 'del':
      return <del key={key}>{inner}</del>;
    case 'sup':
      return <sup key={key}>{inner}</sup>;
    case 'sub':
      return <sub key={key}>{inner}</sub>;
    case 'code':
      return (
        <code key={key} dir="ltr">
          {node.text ?? ''}
        </code>
      );
    case 'br':
      return <br key={key} />;
    case 'hr':
      return <hr key={key} />;
    case 'a': {
      const href = node.href;
      if (href?.startsWith('https://') !== true) return <span key={key}>{inner}</span>;
      return (
        <a
          key={key}
          href={href}
          onClick={(event) => {
            event.preventDefault();
            onLink(href);
          }}
        >
          {inner}
        </a>
      );
    }
    case 'img': {
      const src = ourPicture(node.href);
      if (src === null) return node.text ?? null;
      return (
        <img
          key={key}
          className={styles.inlinePicture}
          src={src}
          alt={node.text ?? ''}
          loading="lazy"
          referrerPolicy="no-referrer"
        />
      );
    }
    default:
      return <span key={key}>{inner}</span>;
  }
}

/** A project's description or changelog, drawn from safe nodes only. */
export function RichText({ nodes, onLink }: Props) {
  return <div className={styles.rich}>{children(nodes, onLink)}</div>;
}
