import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { MyReviews, ReviewFound } from '../../lib/contracts';
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

function mock(answers: Record<string, unknown>) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    return cmd in answers ? answers[cmd] : null;
  });
  return calls;
}

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
