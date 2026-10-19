import type { ReactNode } from 'react';
import type { NoteBlock, NoteRun } from '../../lib/contracts';
import { pictureUrl } from './pictureUrl';
import styles from './Feed.module.css';

interface NoteBlocksProps {
  blocks: NoteBlock[];
  /** Opens a link of the note in the browser (the core checks the address). */
  onLink: (url: string) => void;
  /** The heading level the note's own title has; its headings go below it. */
  titleLevel?: number;
}

/** One run of text: bold, italic, code or a link. Always text, never markup. */
function Run({ run, onLink }: { run: NoteRun; onLink: (url: string) => void }) {
  let content: ReactNode = run.text;
  if (run.code === true) content = <code>{content}</code>;
  if (run.i === true) content = <em>{content}</em>;
  if (run.b === true) content = <strong>{content}</strong>;
  const href = run.href;
  if (href !== undefined) {
    // A button, not an address: nothing in the window navigates, and the core
    // decides whether the browser may open it.
    return (
      <button
        type="button"
        className={styles.link}
        title={href}
        onClick={() => {
          onLink(href);
        }}
      >
        {content}
      </button>
    );
  }
  return <>{content}</>;
}

function Runs({ runs, onLink }: { runs: NoteRun[]; onLink: (url: string) => void }) {
  return (
    <>
      {runs.map((run, index) => (
        <Run key={index} run={run} onLink={onLink} />
      ))}
    </>
  );
}

const HEADINGS = ['h3', 'h4', 'h5', 'h6'] as const;

/**
 * A note's blocks (D-067) as React elements: headings, paragraphs, lists and
 * the feed's own pictures. No HTML is ever read from the note.
 */
export function NoteBlocks({ blocks, onLink, titleLevel = 2 }: NoteBlocksProps) {
  return (
    <>
      {blocks.map((block, index) => {
        switch (block.t) {
          case 'h': {
            const level = Math.min(Math.max(Math.round(block.level), 1), 4);
            const Heading = HEADINGS[Math.min(level + titleLevel - 3, HEADINGS.length - 1)] ?? 'h6';
            return (
              <Heading key={index} className={styles.noteHeading}>
                <Runs runs={block.c} onLink={onLink} />
              </Heading>
            );
          }
          case 'p':
            return (
              <p key={index}>
                <Runs runs={block.c} onLink={onLink} />
              </p>
            );
          case 'ul':
          case 'ol': {
            const List = block.t;
            return (
              <List key={index} className={styles.noteList}>
                {block.items.map((item, itemIndex) => (
                  <li key={itemIndex}>
                    <NoteBlocks blocks={item} onLink={onLink} titleLevel={titleLevel} />
                  </li>
                ))}
              </List>
            );
          }
          case 'img': {
            const src = pictureUrl(block.src);
            return src === null ? null : (
              <img
                key={index}
                className={styles.notePicture}
                src={src}
                alt={block.alt}
                loading="lazy"
              />
            );
          }
          default:
            return null;
        }
      })}
    </>
  );
}
