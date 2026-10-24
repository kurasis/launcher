import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { Delivery, MyReviews, ReviewFound, SentReview } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { onOpenReview, openReview } from './openReview';
import { ReviewsView } from './ReviewsView';

type Call = { cmd: string; args: Record<string, unknown> };

const logs: ReviewFound = {
  files: [
    { name: 'launcher.log', kind: 'text', text: 'INFO Setting user: <player>' },
    { name: 'output.log', kind: 'text', text: 'java.lang.OutOfMemoryError' },
  ],
  removed: { chatLines: 0, names: 1, addresses: 0, worlds: 0, ids: 0 },
};

const none: MyReviews = { reviews: [], unseen: 0, waiting: 0 };

/** An answer, or a function of the arguments that gives one (a promise to hold it). */
function mock(answers: Record<string, unknown>) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    const answer = answers[cmd];
    return typeof answer === 'function'
      ? (answer as (a: unknown) => unknown)(args)
      : (answer ?? null);
  });
  return calls;
}

function review(id: number, more: Partial<SentReview> = {}): SentReview {
  return {
    id,
    sentAt: '2026-10-20T10:00:00Z',
    kind: 'bug',
    category: 'launch',
    subcategory: 'crashes',
    subject: null,
    excerpt: `Review number ${String(id)}`,
    account: false,
    status: 'sent',
    reply: null,
    replied: null,
    replySeen: true,
    ...more,
  };
}

const ANSWERED = review(12, {
  status: 'answered',
  reply: 'Fixed in 0.9.2',
  replied: '2026-10-21T10:00:00Z',
  replySeen: false,
});

const MESSAGE = 'The game closes when I open a world with Sodium.';

describe('ReviewsView', () => {
  it('sends a problem with the message, the place and the chosen logs', async () => {
    window.localStorage.clear();
    const calls = mock({ reviews_mine: none, reviews_logs: logs, reviews_send: 'sent' });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);

    expect(screen.getByRole('radio', { name: 'A problem' })).toBeChecked();
    // Logs are attached to a problem unless the player says otherwise.
    expect(await screen.findByText('launcher.log')).toBeVisible();
    expect(
      screen.getByText('Removed: 0 chat lines, 1 names, 0 addresses, 0 world names, 0 IDs.'),
    ).toBeVisible();
    const send = screen.getByRole('button', { name: 'Send' });
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'Too short' } });
    expect(send).toBeDisabled();
    expect(screen.getByText(/11 more characters needed/)).toBeVisible();

    fireEvent.change(screen.getByLabelText('Message'), { target: { value: `  ${MESSAGE}  ` } });
    fireEvent.change(screen.getByLabelText('Subject (optional)'), {
      target: { value: '  Mods clash with Sodium ' },
    });
    fireEvent.change(screen.getByLabelText('About'), { target: { value: 'mods' } });
    expect(screen.getByLabelText('What exactly')).toHaveValue('catalog');
    fireEvent.change(screen.getByLabelText('What exactly'), { target: { value: 'conflict' } });
    fireEvent.click(screen.getByRole('checkbox', { name: 'launcher.log' }));
    fireEvent.click(send);

    expect(await screen.findByText(/The review is sent/)).toBeVisible();
    expect(calls.find((c) => c.cmd === 'reviews_send')?.args).toEqual({
      draft: {
        kind: 'bug',
        category: 'mods',
        subcategory: 'conflict',
        subject: 'Mods clash with Sodium',
        message: MESSAGE,
        rating: null,
        linkAccount: false,
        diagnosticId: null,
        ad: null,
        files: [logs.files[1]],
      },
    });
    // A sent review leaves no draft behind.
    expect(window.localStorage.getItem('launcher.reviewDraft')).toBeNull();
  });

  it('takes text files dropped on the field, scrubbed by the core', async () => {
    const calls = mock({
      reviews_mine: none,
      reviews_text_file: {
        files: [{ name: 'options.txt', kind: 'text', text: 'lastServer:<address>' }],
        removed: { chatLines: 0, names: 0, addresses: 1, worlds: 0, ids: 0 },
      },
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    fireEvent.click(screen.getByRole('radio', { name: 'An idea' }));
    const field = screen.getByText('Drag pictures and text files here').parentElement;
    if (field === null) throw new Error('no drop field');
    const file = new File(['lastServer:play.example.net'], 'options.txt', { type: 'text/plain' });
    fireEvent.dragOver(field, { dataTransfer: { types: ['Files'], files: [file] } });
    expect(field).toHaveAttribute('data-over', 'true');
    fireEvent.drop(field, { dataTransfer: { types: ['Files'], files: [file] } });

    expect(await screen.findByText('options.txt')).toBeInTheDocument();
    expect(field).toHaveAttribute('data-over', 'false');
    expect(calls.find((c) => c.cmd === 'reviews_text_file')?.args).toEqual({
      name: 'options.txt',
      text: 'lastServer:play.example.net',
    });
    expect(screen.getByRole('button', { name: 'from screenshots' })).toBeEnabled();
  });

  it('takes what the crash panel knows and finds that run’s logs', async () => {
    const calls = mock({ reviews_mine: none, reviews_logs: logs, reviews_send: 'queued' });
    render(
      <ReviewsView
        snapshot={snapshotFixture()}
        prefill={{
          kind: 'bug',
          category: 'launch',
          subcategory: 'crashes',
          instanceId: 'i-1',
          runId: 'r-1',
        }}
        onUnseen={() => undefined}
      />,
    );
    expect(screen.getByLabelText('About')).toHaveValue('launch');
    expect(screen.getByLabelText('What exactly')).toHaveValue('crashes');
    await screen.findByText('output.log');
    expect(calls.find((c) => c.cmd === 'reviews_logs')?.args).toEqual({
      instanceId: 'i-1',
      runId: 'r-1',
    });
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: MESSAGE } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(await screen.findByText(/saved and will be sent by itself/)).toBeVisible();
  });

  it('rates only an impression, and attaches no logs to it', async () => {
    const calls = mock({ reviews_mine: none, reviews_logs: logs, reviews_send: 'sent' });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    expect(screen.queryByText('Your rating')).toBeNull();
    fireEvent.click(screen.getByRole('radio', { name: 'An impression' }));
    expect(screen.getByRole('checkbox', { name: 'Attach the latest logs' })).not.toBeChecked();
    fireEvent.click(screen.getByRole('radio', { name: '4 stars' }));
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: MESSAGE } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    await screen.findByText(/The review is sent/);
    const draft = calls.find((c) => c.cmd === 'reviews_send')?.args.draft as Record<
      string,
      unknown
    >;
    expect(draft).toMatchObject({ kind: 'impression', rating: 4, files: [] });
  });

  it('shows answers, marks them read and deletes a review after asking', async () => {
    const mine: MyReviews = {
      reviews: [
        {
          id: 12,
          sentAt: '2026-10-20T10:00:00Z',
          kind: 'bug',
          category: 'launch',
          subcategory: 'crashes',
          subject: 'Crash with shaders',
          excerpt: 'The game closes…',
          account: false,
          status: 'answered',
          reply: 'Fixed in 0.9.2',
          replied: '2026-10-21T10:00:00Z',
          replySeen: false,
        },
      ],
      unseen: 1,
      waiting: 1,
    };
    const calls = mock({ reviews_mine: mine, reviews_seen: null, reviews_delete: null });
    const onUnseen = vi.fn();
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={onUnseen} />);
    fireEvent.click(await screen.findByRole('tab', { name: 'My reviews (1 new answer)' }));

    expect(await screen.findByText('Fixed in 0.9.2')).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Crash with shaders' })).toBeVisible();
    expect(
      screen.getByText(/^A problem · Starting the game · The game closes or crashes · /),
    ).toBeVisible();
    expect(screen.getByText('Answered')).toBeVisible();
    expect(screen.getByText('1 review waits to be sent.')).toBeVisible();
    await waitFor(() => {
      expect(onUnseen).toHaveBeenLastCalledWith(0);
    });
    expect(calls.some((c) => c.cmd === 'reviews_mine' && c.args.refresh === true)).toBe(true);
    expect(calls.some((c) => c.cmd === 'reviews_seen')).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(calls.some((c) => c.cmd === 'reviews_delete')).toBe(false);
    expect(screen.getByText(/Delete this review/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'reviews_delete')?.args).toEqual({ id: 12 });
    });
  });
});

describe('ReviewsView in concept A', () => {
  it('lists the latest reviews beside the form, without reading their answers', async () => {
    const reviews = [
      ANSWERED,
      review(11, { subject: 'Crash with shaders' }),
      ...[10, 9, 8, 7].map((id) => review(id)),
    ];
    const calls = mock({
      reviews_mine: { reviews, unseen: 1, waiting: 1 },
      reviews_logs: logs,
      reviews_seen: null,
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);

    const recent = await screen.findByRole('region', { name: /^My reviews/ });
    // Six kept and one waiting; the newest five listed.
    expect(within(recent).getByRole('heading', { name: 'My reviews 7' })).toBeVisible();
    expect(within(recent).getByText('1 review waits to be sent.')).toBeVisible();
    const open = within(recent).getAllByRole('button', { name: 'Open' });
    expect(open).toHaveLength(5);
    expect(open[0]).toHaveAccessibleDescription(
      /^The game closes or crashes Review 12 · \u2068.+\u2069$/,
    );
    // A subject the player wrote names the review.
    expect(open[1]).toHaveAccessibleDescription(/^Crash with shaders Review 11 · /);
    expect(within(recent).getByText('Answered')).toBeVisible();
    expect(within(recent).getByText('New')).toBeVisible();
    expect(within(recent).getByText('Answer: “\u2068Fixed in 0.9.2\u2069”')).toBeVisible();
    // An answer shown here is still new: it is read under My reviews.
    expect(calls.some((c) => c.cmd === 'reviews_seen')).toBe(false);
    expect(screen.getByRole('tab', { name: 'My reviews (1 new answer)' })).toBeVisible();

    const all = within(recent).getByRole('button', { name: 'All my reviews' });
    all.focus();
    fireEvent.click(all);
    const mineTab = screen.getByRole('tab', { name: /^My reviews/ });
    expect(mineTab).toHaveAttribute('aria-selected', 'true');
    await waitFor(() => {
      expect(mineTab).toHaveFocus();
    });
  });

  it('opens a review from beside the form under My reviews, focused', async () => {
    mock({
      reviews_mine: { reviews: [review(14), ANSWERED, review(10)], unseen: 1, waiting: 0 },
      reviews_logs: logs,
      reviews_seen: null,
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    const recent = await screen.findByRole('region', { name: /^My reviews/ });
    const open = within(recent).getAllByRole('button', { name: 'Open' })[1];
    if (open === undefined) throw new Error('no second review');
    open.focus();
    fireEvent.click(open);

    const panel = screen.getByRole('tabpanel', { name: /^My reviews/ });
    const heading = within(panel).getAllByRole('heading', { level: 3 })[1];
    await waitFor(() => {
      expect(heading).toHaveFocus();
    });
    expect(heading?.closest('li')).toHaveTextContent('Fixed in 0.9.2');
    // The panel beside the form is not on this tab.
    expect(screen.queryByRole('region', { name: /^My reviews/ })).toBeNull();
    // The answer read here keeps its mark while the list is open.
    expect(within(panel).getByText('New')).toBeVisible();
  });

  it('keeps Send focused while the review goes, then hands the focus to My reviews', async () => {
    window.localStorage.clear();
    let deliver: (delivery: Delivery) => void = () => undefined;
    const calls = mock({
      reviews_mine: none,
      reviews_logs: logs,
      reviews_send: () =>
        new Promise<Delivery>((resolve) => {
          deliver = resolve;
        }),
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    await screen.findByText('launcher.log');
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: MESSAGE } });
    const send = screen.getByRole('button', { name: 'Send' });
    send.focus();
    fireEvent.click(send);
    await waitFor(() => {
      expect(send).toHaveAttribute('aria-disabled', 'true');
    });
    expect(send).toHaveAccessibleName('Sending…');
    expect(send).toBeEnabled();
    expect(send).toHaveFocus();
    fireEvent.click(send);
    expect(calls.filter((c) => c.cmd === 'reviews_send')).toHaveLength(1);

    deliver('sent');
    const mine = await screen.findByRole('button', { name: 'My reviews' });
    await waitFor(() => {
      expect(mine).toHaveFocus();
    });
  });

  it('shows what is attached as chips, each shown on request and removed by its ×', async () => {
    mock({
      reviews_mine: none,
      reviews_text_file: {
        files: [{ name: 'options.txt', kind: 'text', text: 'lastServer:<address>' }],
        removed: { chatLines: 0, names: 0, addresses: 1, worlds: 0, ids: 0 },
      },
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    fireEvent.click(screen.getByRole('radio', { name: 'An idea' }));
    const field = screen.getByText('Drag pictures and text files here').parentElement;
    if (field === null) throw new Error('no drop field');
    const file = new File(['lastServer:play.example.net'], 'options.txt', { type: 'text/plain' });
    fireEvent.drop(field, { dataTransfer: { types: ['Files'], files: [file] } });

    const name = await screen.findByRole('button', { name: 'options.txt' });
    expect(name).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('lastServer:<address>')).toBeNull();
    fireEvent.click(name);
    expect(name).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('lastServer:<address>')).toBeVisible();

    const remove = screen.getByRole('button', { name: 'Remove \u2068options.txt\u2069' });
    remove.focus();
    fireEvent.click(remove);
    expect(screen.queryByRole('button', { name: 'options.txt' })).toBeNull();
    expect(screen.queryByText('lastServer:<address>')).toBeNull();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'choose files' })).toHaveFocus();
    });
  });

  it('gives the focus back to Delete when its question is cancelled, and deletes once', async () => {
    let current: MyReviews = { reviews: [ANSWERED, review(10)], unseen: 1, waiting: 0 };
    let finish: () => void = () => undefined;
    const calls = mock({
      reviews_mine: () => current,
      reviews_seen: null,
      reviews_delete: () =>
        new Promise<null>((resolve) => {
          finish = () => {
            current = { reviews: [review(10)], unseen: 0, waiting: 0 };
            resolve(null);
          };
        }),
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    fireEvent.click(await screen.findByRole('tab', { name: 'My reviews (1 new answer)' }));
    await screen.findByText('Fixed in 0.9.2');

    const remove = screen.getAllByRole('button', { name: 'Delete' })[0];
    if (remove === undefined) throw new Error('no Delete');
    expect(remove).toHaveAccessibleDescription(
      /^A problem · Starting the game · The game closes or crashes Review 12 · /,
    );
    remove.focus();
    fireEvent.click(remove);
    const question = screen.getByRole('alertdialog');
    await waitFor(() => {
      expect(question).toHaveFocus();
    });
    fireEvent.click(within(question).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(remove).toHaveFocus();
    });

    fireEvent.click(remove);
    const asked = screen.getByRole('alertdialog');
    const yes = within(asked).getByRole('button', { name: 'Delete' });
    fireEvent.click(yes);
    fireEvent.click(yes);
    await waitFor(() => {
      expect(yes).toHaveAttribute('aria-disabled', 'true');
    });
    // While it goes, the other reviews wait.
    const other = screen.getByText('Review number 10').closest('li');
    if (other === null) throw new Error('no other review');
    expect(within(other).getByRole('button', { name: 'Delete' })).toHaveAttribute(
      'aria-disabled',
      'true',
    );
    fireEvent.keyDown(asked, { key: 'Escape' });
    expect(screen.getByRole('alertdialog')).toBe(asked);
    expect(calls.filter((c) => c.cmd === 'reviews_delete')).toHaveLength(1);

    finish();
    await waitFor(() => {
      expect(screen.queryByText('Fixed in 0.9.2')).toBeNull();
    });
    await waitFor(() => {
      expect(screen.getByRole('list', { name: 'My reviews' })).toHaveFocus();
    });
  });

  it('says when the reviews cannot be read, each time anew, and tries again', async () => {
    let fail = true;
    let hold = false;
    let release: () => void = () => undefined;
    const broken = () => new Error('The data folder is not available.');
    mock({
      reviews_mine: () => {
        if (!hold) return fail ? Promise.reject(broken()) : none;
        return new Promise((resolve, reject) => {
          release = () => {
            if (fail) reject(broken());
            else resolve(none);
          };
        });
      },
      reviews_logs: logs,
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    fireEvent.click(screen.getByRole('tab', { name: 'My reviews' }));
    const first = await screen.findByRole('alert');
    expect(first).toHaveTextContent('The data folder is not available.');
    await waitFor(() => {
      expect(screen.getByRole('status')).toBeEmptyDOMElement();
    });

    // Pressed, the last answer is not said again while the next one comes.
    hold = true;
    const again = screen.getByRole('button', { name: 'Try again' });
    again.focus();
    fireEvent.click(again);
    expect(screen.queryByRole('alert')).toBeNull();
    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('Checking for answers…');
    });
    expect(again).toHaveAttribute('aria-disabled', 'true');
    expect(again).toHaveFocus();
    release();
    // A second failure is said again, as a new alert.
    const second = await screen.findByRole('alert');
    expect(second).not.toBe(first);
    expect(second).toHaveTextContent('The data folder is not available.');

    fail = false;
    hold = false;
    fireEvent.click(again);
    expect(await screen.findByText('You have not sent any reviews yet.')).toBeVisible();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('starts a new form once a sent review is left, and says how it went', async () => {
    window.localStorage.clear();
    mock({ reviews_mine: none, reviews_logs: logs, reviews_send: 'queued' });
    render(
      <ReviewsView
        snapshot={snapshotFixture()}
        prefill={{ kind: 'idea', category: 'mods', subcategory: 'suggest' }}
        onUnseen={() => undefined}
      />,
    );
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: MESSAGE } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    const mine = await screen.findByRole('button', { name: 'My reviews' });
    // The focus comes to this button, so it carries what happened.
    expect(mine).toHaveAccessibleDescription(/saved and will be sent by itself/);

    mine.focus();
    fireEvent.click(mine);
    const mineTab = screen.getByRole('tab', { name: 'My reviews' });
    expect(mineTab).toHaveAttribute('aria-selected', 'true');
    await waitFor(() => {
      expect(mineTab).toHaveFocus();
    });

    // A mouse press in macOS WebKit leaves the focus on the page.
    mineTab.blur();
    const newTab = screen.getByRole('tab', { name: 'New review' });
    fireEvent.click(newTab);
    await waitFor(() => {
      expect(newTab).toHaveFocus();
    });
    // An empty form: what opened the screen went with the first review.
    expect(screen.getByLabelText('Message')).toHaveValue('');
    expect(screen.getByRole('radio', { name: 'A problem' })).toBeChecked();
    expect(screen.queryByText(/saved and will be sent by itself/)).toBeNull();
  });

  it('takes no more text files than the limit from one drop', async () => {
    const calls = mock({
      reviews_mine: none,
      reviews_text_file: (args: unknown) => ({
        files: [{ name: (args as { name: string }).name, kind: 'text', text: 'renderDistance:12' }],
        removed: { chatLines: 0, names: 0, addresses: 0, worlds: 0, ids: 0 },
      }),
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    fireEvent.click(screen.getByRole('radio', { name: 'An idea' }));
    const field = screen.getByText('Drag pictures and text files here').parentElement;
    if (field === null) throw new Error('no drop field');
    const files = [1, 2, 3, 4, 5, 6, 7].map(
      (n) => new File(['renderDistance:12'], `config-${String(n)}.txt`, { type: 'text/plain' }),
    );
    fireEvent.drop(field, { dataTransfer: { types: ['Files'], files } });

    expect(await screen.findByText('At most 5 text files.')).toBeVisible();
    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(5);
    expect(calls.filter((c) => c.cmd === 'reviews_text_file')).toHaveLength(5);
  });

  it('says it checks for answers in a region that was there before', async () => {
    let release: () => void = () => undefined;
    mock({
      reviews_mine: (args: unknown) =>
        (args as { refresh: boolean }).refresh
          ? new Promise((resolve) => {
              release = () => {
                resolve({ reviews: [ANSWERED], unseen: 1, waiting: 0 });
              };
            })
          : { reviews: [ANSWERED], unseen: 1, waiting: 0 },
      reviews_logs: logs,
      reviews_seen: null,
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    const mineTab = await screen.findByRole('tab', { name: 'My reviews (1 new answer)' });
    const panel = document.getElementById(mineTab.getAttribute('aria-controls') ?? '');
    const status = panel?.querySelector('[role="status"]');
    expect(status).toBeEmptyDOMElement();
    fireEvent.click(mineTab);
    await waitFor(() => {
      expect(status).toHaveTextContent('Checking for answers…');
    });
    expect(screen.getByRole('status')).toBe(status);
    release();
    await waitFor(() => {
      expect(status).toBeEmptyDOMElement();
    });
  });

  it('reads again when a newer read came first, so only answers shown count as read', async () => {
    window.localStorage.clear();
    const asked = review(12);
    let saved = false;
    let check: () => void = () => undefined;
    let deliver: (delivery: Delivery) => void = () => undefined;
    const calls = mock({
      reviews_mine: (args: unknown) => {
        if (!(args as { refresh: boolean }).refresh) {
          return { reviews: [saved ? ANSWERED : asked], unseen: saved ? 1 : 0, waiting: 0 };
        }
        return new Promise((resolve) => {
          check = () => {
            // The core keeps what the service said before it answers.
            saved = true;
            resolve({ reviews: [ANSWERED], unseen: 1, waiting: 0 });
          };
        });
      },
      reviews_logs: logs,
      reviews_seen: null,
      reviews_send: () =>
        new Promise<Delivery>((resolve) => {
          deliver = resolve;
        }),
    });
    render(<ReviewsView snapshot={snapshotFixture()} prefill={null} onUnseen={() => undefined} />);
    await screen.findByText('launcher.log');
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: MESSAGE } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'reviews_send')).toBe(true);
    });
    // My reviews opens while the review goes; the review then arrives, and reads the
    // list again before the service's answer is in.
    fireEvent.click(screen.getByRole('tab', { name: 'My reviews' }));
    deliver('sent');
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'reviews_mine')).toHaveLength(3);
    });
    await screen.findByText('Review number 12');
    check();

    expect(await screen.findByText('Fixed in 0.9.2')).toBeVisible();
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'reviews_seen')).toHaveLength(1);
    });
  });
});

describe('openReview', () => {
  it('reaches the listener with what the caller knows', () => {
    const seen = vi.fn();
    const stop = onOpenReview(seen);
    openReview({ kind: 'bug', category: 'feed', subcategory: 'server_ads', ad: 7 });
    stop();
    openReview();
    expect(seen).toHaveBeenCalledTimes(1);
    expect(seen).toHaveBeenCalledWith({
      kind: 'bug',
      category: 'feed',
      subcategory: 'server_ads',
      ad: 7,
    });
  });
});
