#pragma once

#include <type_traits>

#include "monte_carlo_mean.hpp"

namespace kiyosi::detail {

inline constexpr int monte_carlo_reduction_threads = 256;

template <typename Input, typename Output>
__global__ void reduce_payoffs(const Input* values, int count, Output* output)
{
    __shared__ MonteCarloMean means[monte_carlo_reduction_threads]; // NOLINT(modernize-avoid-c-arrays): CUDA shared memory needs device-accessible array storage.
    const int thread = static_cast<int>(threadIdx.x);
    MonteCarloMean mean{};
    for (int index = static_cast<int>(blockIdx.x * blockDim.x) + thread;
         index < count; index += static_cast<int>(gridDim.x * blockDim.x)) {
        if constexpr (std::is_same_v<Input, MonteCarloMean>)
            mean.merge(values[index]);
        else
            mean.add(values[index]);
    }
    means[thread] = mean;
    __syncthreads();
    for (int offset = monte_carlo_reduction_threads / 2; offset > 0; offset /= 2) {
        if (thread < offset) means[thread].merge(means[thread + offset]);
        __syncthreads();
    }
    if (thread == 0) {
        if constexpr (std::is_same_v<Output, MonteCarloMean>)
            output[blockIdx.x] = means[0];
        else
            *output = means[0].value();
    }
}

} // namespace kiyosi::detail
