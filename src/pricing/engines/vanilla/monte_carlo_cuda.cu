#include "monte_carlo_cuda.hpp"

#include <cmath>
#include <cstddef>
#include <memory>

#include <cuda_runtime.h>
#include <curand_kernel.h>

namespace kiyosi::detail {
namespace {

constexpr int threads_per_block = 256;

struct DeviceDeleter {
    void operator()(void* pointer) const noexcept
    {
        if (pointer != nullptr) cudaFree(pointer);
    }
};

using DeviceMemory = std::unique_ptr<void, DeviceDeleter>;

CudaPricingResult error_result(cudaError_t error)
{
    if (error == cudaErrorMemoryAllocation)
        return {CudaPricingStatus::out_of_memory, 0.0, cudaGetErrorString(error)};
    return {CudaPricingStatus::failure, 0.0, cudaGetErrorString(error)};
}

CudaPricingResult allocate(DeviceMemory& memory, std::size_t size)
{
    void* pointer = nullptr;
    const cudaError_t status = cudaMalloc(&pointer, size);
    if (status != cudaSuccess) return error_result(status);
    memory.reset(pointer);
    return {CudaPricingStatus::success, 0.0, nullptr};
}

__device__ double payoff(int sign, double spot, double strike)
{
    return fmax(static_cast<double>(sign) * (spot - strike), 0.0);
}

__global__ void simulate_payoff_pairs(CudaEuropeanRequest request, int pair_count,
                                      double* payoffs, int* invalid)
{
    const int pair = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (pair >= pair_count) return;

    curandStatePhilox4_32_10_t generator;
    curand_init(request.seed, static_cast<unsigned long long>(pair), 0, &generator);
    double positive_spot = request.spot;
    double negative_spot = request.spot;
    for (int step = 1; step < request.step_count; ++step) {
        const double normal = curand_normal_double(&generator);
        positive_spot *= exp(request.drift + request.diffusion * normal);
        negative_spot *= exp(request.drift - request.diffusion * normal);
        if (!isfinite(positive_spot) || positive_spot <= 0.0 ||
            !isfinite(negative_spot) || negative_spot <= 0.0) {
            atomicExch(invalid, 1);
            payoffs[pair] = 0.0;
            return;
        }
    }
    payoffs[pair] = payoff(request.payoff_sign, positive_spot, request.strike) +
                    payoff(request.payoff_sign, negative_spot, request.strike);
}

__global__ void reduce_payoffs(const double* payoffs, int count, double* total)
{
    __shared__ double sums[threads_per_block];
    const int thread = static_cast<int>(threadIdx.x);
    double sum = 0.0;
    for (int index = thread; index < count; index += threads_per_block)
        sum += payoffs[index];
    sums[thread] = sum;
    __syncthreads();

    for (int offset = threads_per_block / 2; offset > 0; offset /= 2) {
        if (thread < offset) sums[thread] += sums[thread + offset];
        __syncthreads();
    }
    if (thread == 0) *total = sums[0];
}

} // namespace

CudaPricingResult cuda_european_price(CudaEuropeanRequest request)
{
    int device_count = 0;
    cudaError_t status = cudaGetDeviceCount(&device_count);
    if (status == cudaErrorNoDevice || status == cudaErrorInsufficientDriver)
        return {CudaPricingStatus::unavailable, 0.0, cudaGetErrorString(status)};
    if (status != cudaSuccess) return error_result(status);
    if (device_count == 0)
        return {CudaPricingStatus::unavailable, 0.0, "No CUDA device is available"};
    status = cudaSetDevice(0);
    if (status != cudaSuccess) return error_result(status);

    const int pair_count = request.path_count / 2;
    DeviceMemory payoffs;
    DeviceMemory invalid;
    DeviceMemory total;
    auto allocation = allocate(
        payoffs, static_cast<std::size_t>(pair_count) * sizeof(double));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(invalid, sizeof(int));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(total, sizeof(double));
    if (allocation.status != CudaPricingStatus::success) return allocation;

    status = cudaMemset(invalid.get(), 0, sizeof(int));
    if (status != cudaSuccess) return error_result(status);
    const int block_count = (pair_count + threads_per_block - 1) / threads_per_block;
    simulate_payoff_pairs<<<block_count, threads_per_block>>>(
        request, pair_count, static_cast<double*>(payoffs.get()), static_cast<int*>(invalid.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    reduce_payoffs<<<1, threads_per_block>>>(
        static_cast<const double*>(payoffs.get()), pair_count, static_cast<double*>(total.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);

    int invalid_result = 0;
    status = cudaMemcpy(&invalid_result, invalid.get(), sizeof(int), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) return error_result(status);
    if (invalid_result != 0)
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo simulation produced a non-finite path"};

    double payoff_sum = 0.0;
    status = cudaMemcpy(&payoff_sum, total.get(), sizeof(double), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) return error_result(status);
    if (!std::isfinite(payoff_sum))
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo reduction produced a non-finite result"};
    return {CudaPricingStatus::success, payoff_sum, nullptr};
}

} // namespace kiyosi::detail
