use crate::util::{to_extended, ComplexFixed};
use crate::util::complex_extended::ComplexExtended;
use crate::math::reference::Reference;
use crate::util::FloatArbitrary;
use crate::util::float_extended::FloatExtended;
use rayon::prelude::*;

use std::cmp::max;

use atomic_counter::{AtomicCounter, RelaxedCounter};

use std::sync::Arc;

pub struct SeriesApproximation {
    pub maximum_iteration: usize,
    pub delta_pixel_square: FloatExtended,
    pub order: usize,
    pub coefficients: Vec<Vec<ComplexExtended>>,
    probe_start: Vec<ComplexExtended>,
    approximation_probes: Vec<Vec<ComplexExtended>>,
    approximation_probes_derivative: Vec<Vec<ComplexExtended>>,
    pub min_valid_iteration: usize,
    pub max_valid_iteration: usize,
    pub valid_iterations: Vec<usize>,
    pub valid_interpolation: Vec<usize>,
    pub probe_sampling: usize,
    pub experimental: bool,
    pub valid_iteration_frame_multiplier: f32,
    pub valid_iteration_probe_multiplier: f32,
    pub data_storage_interval: usize,
}

impl SeriesApproximation {
    pub fn new_central(order: usize, 
        maximum_iteration: usize, 
        delta_pixel_square: FloatExtended, 
        probe_sampling: usize, 
        experimental: bool, 
        valid_iteration_frame_multiplier: f32, 
        valid_iteration_probe_multiplier: f32,
        data_storage_interval: usize) -> Self {

        // The current iteration is set to 1 as we set z = c
        SeriesApproximation {
            maximum_iteration,
            delta_pixel_square,
            order,
            coefficients: Vec::new(),
            probe_start: Vec::new(),
            approximation_probes: Vec::new(),
            approximation_probes_derivative: Vec::new(),
            min_valid_iteration: 1,
            max_valid_iteration: 1,
            valid_iterations: Vec::new(),
            valid_interpolation: Vec::new(),
            probe_sampling,
            experimental,
            valid_iteration_frame_multiplier,
            valid_iteration_probe_multiplier,
            data_storage_interval,
        }
    }

    pub fn generate_approximation(&mut self, center_reference: &Reference, series_approximation_counter: &Arc<RelaxedCounter>, stop_flag: &Arc<RelaxedCounter>) {
        // Reset the coefficients
        self.coefficients = vec![vec![ComplexExtended::new2(0.0, 0.0, 0); self.order as usize + 1]; 1];

        // 1th element is the z^2 + c, which is the 1st iteration
        self.coefficients[0][0] = to_extended(&center_reference.c);
        self.coefficients[0][1] = ComplexExtended::new2(1.0, 0.0, 0);

        let add_value = ComplexExtended::new2(1.0, 0.0, 0);

        let mut previous_coefficients = self.coefficients[0].clone();
        let mut next_coefficients = vec![ComplexExtended::new2(0.0, 0.0, 0); self.order as usize + 1];

        // Can be changed later into a better loop - this function could also return some more information
        // Go through all remaining iterations
        for i in 1..self.maximum_iteration {
            if stop_flag.get() >= 1 {
                return
            };

            // This is checking if the approximation can step forward so takes the next iteration
            next_coefficients[0] = center_reference.reference_data_extended[i];
            next_coefficients[1] = previous_coefficients[0] * previous_coefficients[1] * 2.0 + add_value;
            next_coefficients[0].reduce();
            next_coefficients[1].reduce();

            // Calculate the new coefficents
            for k in 2..=self.order {
                let mut sum = previous_coefficients[0] * previous_coefficients[k];

                for j in 1..=((k - 1) / 2) {
                    sum += previous_coefficients[j] * previous_coefficients[k - j];
                }
                sum *= 2.0;

                // If even, we include the mid term as well
                if k % 2 == 0 {
                    sum += previous_coefficients[k / 2] * previous_coefficients[k / 2];
                }

                sum.reduce();
                next_coefficients[k] = sum;
            }

            previous_coefficients = next_coefficients.clone();

            series_approximation_counter.inc();
            
            // only every 100th iteration (101, 201 etc)
            // This is 0, 100, 200 -> 1, 101, 201
            if i % self.data_storage_interval == 0 {
                self.coefficients.push(next_coefficients.clone());
            }

            // self.coefficients.push(next_coefficients);
        }
    }

    pub fn check_approximation(&mut self, 
        delta_top_left_mantissa: ComplexFixed<f64>, 
        delta_top_left_exponent: i32, 
        cos_rotate: f64, 
        sin_rotate: f64, 
        delta_pixel: f64,
        image_width: usize,
        image_height: usize,
        center_reference: &Reference,
        series_validation_counter: &Arc<RelaxedCounter>) {
        // Delete the previous probes and calculate new ones
        self.probe_start = Vec::new();
        self.approximation_probes = Vec::new();
        self.approximation_probes_derivative = Vec::new();

        // Probes are stored in row first order
        for j in 0..self.probe_sampling {
            for i in 0..self.probe_sampling {

                let pos_x = image_width as f64 * (i as f64 / (self.probe_sampling as f64 - 1.0));
                let pos_y = image_height as f64 * (j as f64 / (self.probe_sampling as f64 - 1.0));

                // This could be changed to account for jittering if needed
                let real = pos_x * delta_pixel * cos_rotate - pos_y * delta_pixel * sin_rotate + delta_top_left_mantissa.re;
                let imag = pos_x * delta_pixel * sin_rotate + pos_y * delta_pixel * cos_rotate + delta_top_left_mantissa.im;

                self.add_probe(ComplexExtended::new2(
                    real, 
                    imag, delta_top_left_exponent));
            }
        }

        // We check using the top left probe
        let i = 0;

        let test_val = max((self.min_valid_iteration as f32 * self.valid_iteration_frame_multiplier) as usize, 1000);

        let mut first_valid_iterations = if self.min_valid_iteration > test_val {
            self.min_valid_iteration - test_val
        } else {
            1
        };

        // println!("{} {} {} {}", self.min_valid_iteration, first_valid_iterations, test_val, self.maximum_iteration);

        // TODO make this adapt for the division (defaults to 1 so same)
        let mut probe = self.evaluate(self.probe_start[i], first_valid_iterations);
        
        while first_valid_iterations < self.maximum_iteration {
            // step the probe points using perturbation
            probe = probe * (center_reference.reference_data_extended[first_valid_iterations - 1] * 2.0 + probe);
            probe += self.probe_start[i];

            // This is not done on every iteration
            if first_valid_iterations % 250 == 0 {
                probe.reduce();
            }

            // triggers on when the next iteration would be 1, 101, 201 etc.
            if first_valid_iterations % self.data_storage_interval == 0 {
                // valid_iteration + 1 - 1 (for the approximation)
                let next_coefficients = &self.coefficients[first_valid_iterations / self.data_storage_interval];

                // get the new approximations
                let mut series_probe = next_coefficients[1] * self.approximation_probes[i][0];
                let mut derivative_probe = next_coefficients[1] * self.approximation_probes_derivative[i][0];

                for k in 2..=self.order {
                    series_probe += next_coefficients[k] * self.approximation_probes[i][k - 1];
                    derivative_probe += next_coefficients[k] * self.approximation_probes_derivative[i][k - 1];
                };

                let relative_error = (probe - series_probe).norm_square();
                let mut derivative = derivative_probe.norm_square();

                // Check to make sure that the derivative is greater than or equal to 1
                if derivative.to_float() < 1.0 {
                    derivative.mantissa = 1.0; 
                    derivative.exponent = 0;
                }

                // The first element is reduced, the second might need to be reduced a little more
                // Check that the error over the derivative is less than the pixel spacing
                if relative_error / derivative > self.delta_pixel_square {
                    // println!("rel: {}, deri: {}, delta: {}", relative_error, derivative, self.delta_pixel_square);

                    first_valid_iterations = if first_valid_iterations > self.data_storage_interval {
                        first_valid_iterations - self.data_storage_interval + 1
                    } else {
                        1
                    };

                    break;
                }
            }

            first_valid_iterations += 1;
        }

        series_validation_counter.inc();

        self.min_valid_iteration = self.data_storage_interval * ((first_valid_iterations - 1) / self.data_storage_interval) + 1;

        // println!("{}", self.min_valid_iteration);

        let test_val = max(
            ((self.min_valid_iteration as f32 * self.valid_iteration_probe_multiplier) as usize / self.data_storage_interval) * self.data_storage_interval, 
            (1000 / self.data_storage_interval) * self.data_storage_interval);

        let mut current_probe_check_value = if self.min_valid_iteration > test_val {
            self.min_valid_iteration - test_val
        } else {
            1
        };

        let mut next_probe_check_value = if current_probe_check_value > test_val {
            current_probe_check_value - test_val
        } else {
            1
        };

        // println!("{} {}", current_probe_check_value, next_probe_check_value);

        // This is the array that will be iterated
        let mut valid_iterations = vec![current_probe_check_value; self.probe_sampling * self.probe_sampling];

        // preallocate array for the values

        loop {
            valid_iterations.par_iter_mut().enumerate()
                .for_each(|(i, probe_iteration_level)| {
                    // check if the probe has already found its max skip
                    if *probe_iteration_level == current_probe_check_value {
                        let mut probe = self.evaluate(self.probe_start[i], *probe_iteration_level);

                        while *probe_iteration_level < self.maximum_iteration {
                            // step the probe points using perturbation
                            probe = probe * (center_reference.reference_data_extended[*probe_iteration_level - 1] * 2.0 + probe);
                            probe += self.probe_start[i];

                            // This is not done on every iteration
                            if *probe_iteration_level % 250 == 0 {
                                probe.reduce();
                            }

                            // triggers on the first iteration when the next iteration is 1001, 1101 etc.
                            if *probe_iteration_level % self.data_storage_interval == 0 {
                                let next_coefficients = &self.coefficients[*probe_iteration_level / self.data_storage_interval];

                                // get the new approximations
                                let mut series_probe = next_coefficients[1] * self.approximation_probes[i][0];
                                let mut derivative_probe = next_coefficients[1] * self.approximation_probes_derivative[i][0];

                                for k in 2..=self.order {
                                    series_probe += next_coefficients[k] * self.approximation_probes[i][k - 1];
                                    derivative_probe += next_coefficients[k] * self.approximation_probes_derivative[i][k - 1];
                                };

                                probe.reduce();

                                let relative_error = (probe - series_probe).norm_square();
                                let mut derivative = derivative_probe.norm_square();

                                // Check to make sure that the derivative is greater than or equal to 1
                                if derivative.to_float() < 1.0 {
                                    derivative.mantissa = 1.0;
                                    derivative.exponent = 0;
                                }

                                // println!("checking at: {}", *probe_iteration_level);
                                // println!("relative error: {} derivative: {} delta_square: {}", relative_error, derivative, self.delta_pixel_square);
                                // println!("probe: {} series_probe: {}", probe, series_probe);

                                // The first element is reduced, the second might need to be reduced a little more
                                // Check that the error over the derivative is less than the pixel spacing
                                if relative_error / derivative > self.delta_pixel_square || relative_error.exponent > 0 {
                                    // println!("exceeded at: {} ", *probe_iteration_level);

                                    if *probe_iteration_level <= (current_probe_check_value + self.data_storage_interval + 1) {
                                        *probe_iteration_level = next_probe_check_value;
                                        break;
                                    };

                                    *probe_iteration_level = if *probe_iteration_level > self.data_storage_interval {
                                        *probe_iteration_level - self.data_storage_interval + 1
                                    } else {
                                        1
                                    };

                                    break;
                                }
                            }

                            *probe_iteration_level += 1;
                        }

                        if *probe_iteration_level == self.maximum_iteration {
                            // 100 -> 0 + 1 = 1, 101 -> 100 + 1 = 101
                            *probe_iteration_level = ((*probe_iteration_level - 1) / self.data_storage_interval) * self.data_storage_interval + 1;
                        }
                    };
                });

            // we have now iterated all the values, we need to update those which skipped too quickly
            self.min_valid_iteration = *valid_iterations.iter().min().unwrap();

            // this would indicate that no more of the probes are bad
            if self.min_valid_iteration != next_probe_check_value  || self.min_valid_iteration == 1 {
                // println!("{:?}, {}, {}, {}", valid_iterations, self.min_valid_iteration, current_probe_check_value, next_probe_check_value);
                break;
            } else {
                current_probe_check_value = next_probe_check_value;

                let test_val = max(
                    ((self.min_valid_iteration as f32 * self.valid_iteration_probe_multiplier) as usize / self.data_storage_interval) * self.data_storage_interval, 
                    (1000 / self.data_storage_interval) * self.data_storage_interval);
    
                next_probe_check_value = if current_probe_check_value > test_val {
                    current_probe_check_value - test_val
                } else {
                    1
                };
                // println!("{:?}, {}, {}, {}", valid_iterations, self.min_valid_iteration, current_probe_check_value, next_probe_check_value);
            }
        }

        series_validation_counter.inc();

        self.valid_iterations = valid_iterations;

        // Also, here we do the interpolation and set up the array
        self.valid_interpolation = Vec::new();

        for j in 0..(self.probe_sampling - 1) {
            for i in 0..(self.probe_sampling - 1) {
                // this is the index into the main array
                let index = j * self.probe_sampling + i;

                let min_interpolation = *[self.valid_iterations[index], 
                    self.valid_iterations[index + 1], 
                    self.valid_iterations[index + self.probe_sampling], 
                    self.valid_iterations[index + self.probe_sampling + 1]].iter().min().unwrap();

                self.valid_interpolation.push(min_interpolation);
            }
        }

        
        self.max_valid_iteration = if self.experimental {
            *self.valid_interpolation.iter().max().unwrap()
        } else {
            self.min_valid_iteration
        };

        // println!("series approximation valid interpolation buffer:");
        // let temp_size = self.probe_sampling - 1;
        // for i in 0..temp_size {
        //     let test = &self.valid_interpolation[(i * temp_size)..((i + 1) * temp_size)];
        //     print!("[");

        //     for element in test {
        //         print!("{:>8},", element);
        //     }

        //     print!("\x08]\n");
        // }

        if !self.experimental {
            self.valid_interpolation = vec![self.min_valid_iteration; (self.probe_sampling - 1) * (self.probe_sampling - 1)];
        }
    }

    pub fn add_probe(&mut self, delta_probe: ComplexExtended) {
        // here we will need to check to make sure we are still at the first iteration, or use perturbation to go forward
        self.probe_start.push(delta_probe);

        let mut current_value = delta_probe;

        let mut delta_n = Vec::with_capacity(self.order + 1);
        let mut delta_derivative_n = Vec::with_capacity(self.order + 1);

        // The first element will be 1, in order for the derivative to be calculated
        delta_n.push(current_value);
        delta_derivative_n.push(ComplexExtended::new2(1.0, 0.0, 0));

        for i in 1..=self.order {
            delta_derivative_n.push(current_value * (i + 1) as f64);
            current_value *= delta_probe;
            delta_n.push(current_value);
        }

        self.approximation_probes.push(delta_n);
        self.approximation_probes_derivative.push(delta_derivative_n);
    }

    // Get the current reference, and the current number of iterations done
    pub fn get_reference(&self, reference_delta: ComplexExtended, center_reference: &Reference) -> Reference {
        let precision = center_reference.c.real().prec();
        // let iteration_reference = self.data_storage_interval * ((self.min_valid_iteration - 1) / self.data_storage_interval) + 1;

        let mut reference_c = center_reference.c.clone();
        let temp = FloatArbitrary::with_val(precision, reference_delta.exponent).exp2();
        let temp2 = FloatArbitrary::with_val(precision, reference_delta.mantissa.re);
        let temp3 = FloatArbitrary::with_val(precision, reference_delta.mantissa.im);

        *reference_c.mut_real() += &temp2 * &temp;
        *reference_c.mut_imag() += &temp3 * &temp;

        // let mut reference_z = self.center_reference.approximation_data[self.valid_iteration].clone();
        let mut reference_z = center_reference.high_precision_data[(self.min_valid_iteration - 1) / self.data_storage_interval].clone();

        let temp4 = self.evaluate(reference_delta, self.min_valid_iteration);
        let temp = FloatArbitrary::with_val(precision, temp4.exponent).exp2();
        let temp2 = FloatArbitrary::with_val(precision, temp4.mantissa.re);
        let temp3 = FloatArbitrary::with_val(precision, temp4.mantissa.im);

        *reference_z.mut_real() += &temp2 * &temp;
        *reference_z.mut_imag() += &temp3 * &temp;

        Reference::new(reference_z, reference_c, self.min_valid_iteration, center_reference.maximum_iteration, self.data_storage_interval, center_reference.glitch_tolerance, center_reference.zoom)
    }

    pub fn evaluate(&self, point_delta: ComplexExtended, iteration: usize) -> ComplexExtended {
        // This could be improved to use the iteration option better
        // this assumes that the requested iteration is a multiple of the data interval

        // 101 -> 100 / 100 = 1, 1 -> 0 / 100 = 0, 201 -> 200 / 100 = 2
        let new_coefficients = &self.coefficients[(iteration - 1) / self.data_storage_interval];
        // Horner's rule
        let mut approximation = new_coefficients[self.order];

        for k in (1..=(self.order - 1)).rev() {
            approximation *= point_delta;
            approximation += new_coefficients[k];
        }

        approximation *= point_delta;
        approximation.reduce();
        approximation
    }

    pub fn evaluate_derivative(&self, point_delta: ComplexExtended, iteration: usize) -> ComplexExtended {
        // This could be improved to use the iteration option better
        // this assumes that the requested iteration is a multiple of the data interval

        // 101 -> 100 / 100 = 1, 1 -> 0 / 100 = 0, 201 -> 200 / 100 = 2
        let new_coefficients = &self.coefficients[(iteration - 1) / self.data_storage_interval];
        // Horner's rule
        let mut approximation = new_coefficients[self.order];
        approximation *= self.order as f64;

        for k in (1..=(self.order - 1)).rev() {
            approximation *= point_delta;
            approximation += new_coefficients[k] * k as f64;
        }

        // println!("{:?}", approximation);

        // approximation *= point_delta;
        approximation.reduce();
        approximation
    }
}