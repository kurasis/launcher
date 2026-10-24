import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { TabPanel, Tabs } from './Tabs';

type Room = 'a' | 'b' | 'c';

function Rooms() {
  const [room, setRoom] = useState<Room>('a');
  return (
    <>
      <Tabs
        label="Rooms"
        id="rooms"
        value={room}
        onChange={setRoom}
        items={[
          { value: 'a', label: 'Alpha' },
          { value: 'b', label: 'Beta' },
          { value: 'c', label: 'Gamma' },
        ]}
      />
      {(['a', 'b', 'c'] as const).map((value) => (
        <TabPanel key={value} id="rooms" value={value} shown={room === value}>
          <p>Room {value}</p>
        </TabPanel>
      ))}
    </>
  );
}

const tab = (name: string) => screen.getByRole('tab', { name });

describe('Tabs', () => {
  it('shows the chosen tab’s panel, named by its tab, and keeps one tab stop', () => {
    render(<Rooms />);
    expect(screen.getByRole('tablist', { name: 'Rooms' })).toBeInTheDocument();
    expect(tab('Alpha')).toHaveAttribute('aria-selected', 'true');
    expect(tab('Alpha')).toHaveAttribute('tabindex', '0');
    expect(tab('Beta')).toHaveAttribute('tabindex', '-1');
    const panel = screen.getByRole('tabpanel', { name: 'Alpha' });
    expect(panel).toHaveTextContent('Room a');
    expect(tab('Alpha')).toHaveAttribute('aria-controls', panel.id);
    // Every tab points at a panel that is there, shown or not.
    expect(document.getElementById(tab('Gamma').getAttribute('aria-controls') ?? '')).toBeTruthy();

    fireEvent.click(tab('Beta'));
    expect(tab('Beta')).toHaveAttribute('aria-selected', 'true');
    expect(tab('Beta')).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('tabpanel', { name: 'Beta' })).toHaveTextContent('Room b');
    expect(screen.queryByRole('tabpanel', { name: 'Alpha' })).toBeNull();
  });

  it('moves with the arrow keys, around the ends, and to the ends with Home and End', () => {
    render(<Rooms />);
    tab('Alpha').focus();
    fireEvent.keyDown(tab('Alpha'), { key: 'ArrowRight' });
    expect(tab('Beta')).toHaveFocus();
    expect(tab('Beta')).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(tab('Beta'), { key: 'End' });
    expect(tab('Gamma')).toHaveFocus();
    fireEvent.keyDown(tab('Gamma'), { key: 'ArrowRight' });
    expect(tab('Alpha')).toHaveFocus();
    fireEvent.keyDown(tab('Alpha'), { key: 'ArrowLeft' });
    expect(tab('Gamma')).toHaveFocus();
    fireEvent.keyDown(tab('Gamma'), { key: 'Home' });
    expect(tab('Alpha')).toHaveFocus();
    expect(tab('Alpha')).toHaveAttribute('aria-selected', 'true');
  });

  it('follows the reading direction in Arabic', () => {
    render(
      <div dir="rtl" style={{ direction: 'rtl' }}>
        <Rooms />
      </div>,
    );
    tab('Alpha').focus();
    // Left is forward when the text runs right to left.
    fireEvent.keyDown(tab('Alpha'), { key: 'ArrowLeft' });
    expect(tab('Beta')).toHaveFocus();
    fireEvent.keyDown(tab('Beta'), { key: 'ArrowRight' });
    expect(tab('Alpha')).toHaveFocus();
  });
});
