// Copyright (c) 2023 The TQUIC Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/*
 * Copyright 2017, Google Inc.
 *
 * Use of this source code is governed by the following BSD-style license:
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are
 * met:
 *
 *    * Redistributions of source code must retain the above copyright
 * notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above
 * copyright notice, this list of conditions and the following disclaimer
 * in the documentation and/or other materials provided with the
 * distribution.
 *
 *    * Neither the name of Google Inc. nor the names of its
 * contributors may be used to endorse or promote products derived from
 * this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
 * A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
 * OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

//! A windowed min/max estimator, which is based on an algorithm by Kathleen Nichols.
//! Refer to <https://groups.google.com/g/bbr-dev/c/3RTgkzi5ZD8>.
//!  * lib/minmax.c: windowed min/max tracker
//!
//! Kathleen Nichols' algorithm for tracking the minimum (or maximum)
//! value of a data stream over some fixed time interval.  (E.g.,
//! the minimum RTT over the past five minutes.) It uses constant
//! space and constant time per update yet almost always delivers
//! the same minimum as an implementation that has to keep all the
//! data in the window.
//!
//! The algorithm keeps track of the best, 2nd best & 3rd best min
//! values, maintaining an invariant that the measurement time of
//! the n'th best >= n-1'th best. It also makes sure that the three
//! values are widely separated in the time window since that bounds
//! the worse case error when that data is monotonically increasing
//! over the window.
//!
//! Upon getting a new min, we can forget everything earlier because
//! it has no value - the new min is <= everything else in the window
//! by definition and it's the most recent. So we restart fresh on
//! every new min and overwrites 2nd & 3rd choices. The same property
//! holds for 2nd & 3rd best.

#![allow(unused_variables)]

#[derive(Debug, Copy, Clone, Default)]
pub (super)struct MinMaxSample {
    /// Round trip count.
    time: u64,

    /// Sample value.
    value: u64,
}

#[derive(Debug,Clone)]
pub (super)struct MinMax {
    /// The max lasting time window to pick up the best sample.
    window: u64,

    /// The best, second best, third best samples.
    samples: [MinMaxSample; 3],
}

impl MinMax {
    pub (super)fn new(window: u64) -> Self {
        Self {
            window,
            samples: [Default::default(); 3],
        }
    }

    /// Set window size.
/*     pub (super)fn set_window(&mut self, window: u64) {
        self.window = window;
    } */

    /// Reset all samples to the given sample.
    pub (super)fn reset(&mut self, sample: MinMaxSample) {
        self.samples.fill(sample)
    }

    /// As time advances, update the 1st, 2nd, and 3rd choices.
    fn subwin_update(&mut self, sample: MinMaxSample) {
        let dt = sample.time.saturating_sub(self.samples[0].time);
        if dt > self.window {
            // Passed entire window without a new sample so make 2nd
            // choice the new sample & 3rd choice the new 2nd choice.
            // we may have to iterate this since our 2nd choice
            // may also be outside the window (we checked on entry
            // that the third choice was in the window).
            self.samples[0] = self.samples[1];
            self.samples[1] = self.samples[2];
            self.samples[2] = sample;
            if sample.time.saturating_sub(self.samples[0].time) > self.window {
                self.samples[0] = self.samples[1];
                self.samples[1] = self.samples[2];
                self.samples[2] = sample;
            }
        } else if self.samples[1].time == self.samples[0].time && dt > self.window / 4_u64 {
            // We've passed a quarter of the window without a new sample
            // so take a 2nd choice from the 2nd quarter of the window.
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if self.samples[2].time == self.samples[1].time && dt > self.window / 2_u64 {
            // We've passed half the window without finding a new sample
            // so take a 3rd choice from the last half of the window
            self.samples[2] = sample;
        }
    }

    /// Check if new measurement updates the 1st, 2nd or 3rd choice max.
    pub (super)fn update_max(&mut self, time: u64, value: u64) {
        if time < self.samples[2].time {
            // Time should be monotonically increasing.
            return;
        }

        let sample = MinMaxSample { time, value };

        if self.samples[0].value == 0  // uninitialized
            || sample.value >= self.samples[0].value // found new max?
            || sample.time.saturating_sub(self.samples[2].time) > self.window
        // nothing left in window?
        {
            self.reset(sample); // forget earlier samples
            return;
        }

        if sample.value >= self.samples[1].value {
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if sample.value >= self.samples[2].value {
            self.samples[2] = sample;
        }

        self.subwin_update(sample);
    }

    /// Check if new measurement updates the 1st, 2nd or 3rd choice min.
   /*  pub (super)fn update_min(&mut self, time: u64, value: u64) {
        if time < self.samples[2].time {
            // Time should be monotonically increasing.
            return;
        }

        let sample = MinMaxSample { time, value };

        if self.samples[0].value == 0  // uninitialised
            || sample.value <= self.samples[0].value // found new min?
            || sample.time.saturating_sub(self.samples[2].time) > self.window
        // nothing left in window?
        {
            self.reset(sample); // forget earlier samples
            return;
        }

        if sample.value <= self.samples[1].value {
            self.samples[2] = sample;
            self.samples[1] = sample;
        } else if sample.value <= self.samples[2].value {
            self.samples[2] = sample;
        }

        self.subwin_update(sample);
    } */

    /// Get the min/max value.
    pub (super)fn get(&self) -> u64 {
        self.samples[0].value
    }
}

impl Default for MinMax {
    fn default() -> Self {
        Self {
            // The default window for BBR is 10 round trips
            window: 10,
            samples: [Default::default(); 3],
        }
    }
}

