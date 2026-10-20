package app.launcher.gametest.core;

import java.util.Arrays;

/** Samples of one number (frames per second, chunks, …): count, minimum, average, low percentile. */
public final class Stats {
    private double[] values = new double[64];
    private int count;

    public void add(double value) {
        if (Double.isNaN(value) || Double.isInfinite(value)) {
            return;
        }
        if (count == values.length) {
            values = Arrays.copyOf(values, count * 2);
        }
        values[count++] = value;
    }

    public int count() {
        return count;
    }

    public double min() {
        double min = Double.NaN;
        for (int i = 0; i < count; i++) {
            min = i == 0 ? values[i] : Math.min(min, values[i]);
        }
        return min;
    }

    public double max() {
        double max = Double.NaN;
        for (int i = 0; i < count; i++) {
            max = i == 0 ? values[i] : Math.max(max, values[i]);
        }
        return max;
    }

    public double avg() {
        if (count == 0) {
            return Double.NaN;
        }
        double sum = 0;
        for (int i = 0; i < count; i++) {
            sum += values[i];
        }
        return sum / count;
    }

    /** The value this share of samples is at or below (0.05 for the worst 5 %). */
    public double percentile(double share) {
        if (count == 0) {
            return Double.NaN;
        }
        double[] sorted = Arrays.copyOf(values, count);
        Arrays.sort(sorted);
        int index = (int) Math.floor(share * (count - 1));
        return sorted[Math.max(0, Math.min(count - 1, index))];
    }
}
