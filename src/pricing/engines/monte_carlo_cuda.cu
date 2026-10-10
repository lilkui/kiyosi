#include "monte_carlo_cuda.hpp"
#include "monte_carlo_mean.hpp"
#include "monte_carlo_cuda_reduction.hpp"
#include "vanilla/monte_carlo_regression.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <memory>
#include <type_traits>

#include <cuda_runtime.h>
#include <curand_kernel.h>

namespace kiyosi::detail {
namespace {

constexpr int threads_per_block = monte_carlo_reduction_threads;

struct RegressionStatistics {
    double sum_x;
    double sum_x2;
    double sum_x3;
    double sum_x4;
    double sum_y;
    double sum_xy;
    double sum_x2y;
    unsigned long long sample_count;
};

struct RegressionSolution {
    double coefficients[3]; // NOLINT(modernize-avoid-c-arrays): shared host/device regression uses device-accessible storage.
    bool valid;
};

struct DeviceDeleter {
    void operator()(void* pointer) const noexcept
    {
        if (pointer != nullptr) cudaFree(pointer);
    }
};

using DeviceMemory = std::unique_ptr<void, DeviceDeleter>;

struct PathSimulationResult {
    double mean;
    int invalid;
};

struct PathOutputs {
    DeviceMemory memory;
    double* payoffs{};
    MonteCarloMean* partials{};
    PathSimulationResult* result{};
};

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

CudaPricingResult check_device_available()
{
    int device_count = 0;
    const cudaError_t status = cudaGetDeviceCount(&device_count);
    if (status == cudaErrorNoDevice || status == cudaErrorInsufficientDriver)
        return {CudaPricingStatus::unavailable, 0.0, cudaGetErrorString(status)};
    if (status != cudaSuccess) return error_result(status);
    if (device_count == 0)
        return {CudaPricingStatus::unavailable, 0.0, "No CUDA device is available"};
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
    const double normal = curand_normal_double(&generator);
    const double positive_spot = request.spot * exp(request.drift + request.diffusion * normal);
    const double negative_spot = request.spot * exp(request.drift - request.diffusion * normal);
    if (!isfinite(positive_spot) || positive_spot <= 0.0 ||
        !isfinite(negative_spot) || negative_spot <= 0.0) {
        atomicExch(invalid, 1);
        payoffs[pair] = 0.0;
        payoffs[pair + pair_count] = 0.0;
        return;
    }
    payoffs[pair] = payoff(request.payoff_sign, positive_spot, request.strike);
    payoffs[pair + pair_count] = payoff(request.payoff_sign, negative_spot, request.strike);
}

__global__ void simulate_american_pairs(CudaAmericanRequest request, int pair_count,
                                        double* paths, int* invalid)
{
    const int pair = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (pair >= pair_count) return;

    const int negative_path = pair + pair_count;
    curandStatePhilox4_32_10_t generator;
    curand_init(request.seed, static_cast<unsigned long long>(pair), 0, &generator);
    double positive_spot = request.spot;
    double negative_spot = request.spot;
    paths[pair] = positive_spot;
    paths[negative_path] = negative_spot;
    for (int step = 1; step < request.step_count; ++step) {
        const double normal = curand_normal_double(&generator);
        positive_spot *= exp(request.drift + request.diffusion * normal);
        negative_spot *= exp(request.drift - request.diffusion * normal);
        if (!isfinite(positive_spot) || positive_spot <= 0.0 ||
            !isfinite(negative_spot) || negative_spot <= 0.0) {
            atomicExch(invalid, 1);
            return;
        }
        const auto offset = static_cast<std::size_t>(step) * request.path_count;
        paths[offset + pair] = positive_spot;
        paths[offset + negative_path] = negative_spot;
    }
}

__global__ void initialize_cash_flows(CudaAmericanRequest request,
                                      const double* paths, double* cash_flows)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= request.path_count) return;
    const auto offset = static_cast<std::size_t>(request.step_count - 1) * request.path_count;
    cash_flows[path] = payoff(request.payoff_sign, paths[offset + path], request.strike);
}

__device__ RegressionStatistics add_statistics(RegressionStatistics left,
                                               RegressionStatistics right)
{
    left.sum_x += right.sum_x;
    left.sum_x2 += right.sum_x2;
    left.sum_x3 += right.sum_x3;
    left.sum_x4 += right.sum_x4;
    left.sum_y += right.sum_y;
    left.sum_xy += right.sum_xy;
    left.sum_x2y += right.sum_x2y;
    left.sample_count += right.sample_count;
    return left;
}

__global__ void discount_and_collect_statistics(
    CudaAmericanRequest request, int step, const double* paths,
    double* cash_flows, RegressionStatistics* partials)
{
    __shared__ RegressionStatistics sums[threads_per_block]; // NOLINT(modernize-avoid-c-arrays): CUDA shared memory needs device-accessible array storage.
    const int thread = static_cast<int>(threadIdx.x);
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    RegressionStatistics local{};
    if (path < request.path_count) {
        const double cash_flow = cash_flows[path] * request.discount;
        cash_flows[path] = cash_flow;
        const auto offset = static_cast<std::size_t>(step) * request.path_count;
        const double spot = paths[offset + path];
        const double intrinsic = payoff(request.payoff_sign, spot, request.strike);
        if (intrinsic > 0.0) {
            const double x = spot / request.strike;
            const double x2 = x * x;
            local = {x, x2, x2 * x, x2 * x2,
                     cash_flow / request.strike, x * (cash_flow / request.strike),
                     x2 * (cash_flow / request.strike), 1};
        }
    }
    sums[thread] = local;
    __syncthreads();

    for (int offset = threads_per_block / 2; offset > 0; offset /= 2) {
        if (thread < offset)
            sums[thread] = add_statistics(sums[thread], sums[thread + offset]);
        __syncthreads();
    }
    if (thread == 0) partials[blockIdx.x] = sums[0];
}

__global__ void reduce_statistics(const RegressionStatistics* partials,
                                  int count, RegressionSolution* solution)
{
    __shared__ RegressionStatistics sums[threads_per_block]; // NOLINT(modernize-avoid-c-arrays): CUDA shared memory needs device-accessible array storage.
    const int thread = static_cast<int>(threadIdx.x);
    RegressionStatistics sum{};
    for (int index = thread; index < count; index += threads_per_block)
        sum = add_statistics(sum, partials[index]);
    sums[thread] = sum;
    __syncthreads();

    for (int offset = threads_per_block / 2; offset > 0; offset /= 2) {
        if (thread < offset)
            sums[thread] = add_statistics(sums[thread], sums[thread + offset]);
        __syncthreads();
    }
    if (thread == 0) {
        const auto statistics = sums[0];
        solution->valid = false;
        if (statistics.sample_count == 0) return;
        const double sample_count = static_cast<double>(statistics.sample_count);
        const double matrix[3][4] = {
            // NOLINT(modernize-avoid-c-arrays): device code cannot call host std::array methods.
            {sample_count, statistics.sum_x, statistics.sum_x2, statistics.sum_y},
            {statistics.sum_x, statistics.sum_x2, statistics.sum_x3, statistics.sum_xy},
            {statistics.sum_x2, statistics.sum_x3, statistics.sum_x4, statistics.sum_x2y},
        };
        solution->valid = solve_quadratic(matrix, solution->coefficients);
    }
}

__global__ void apply_exercise(CudaAmericanRequest request, int step,
                               const double* paths, double* cash_flows,
                               const RegressionSolution* solution)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= request.path_count || !solution->valid) return;
    const auto offset = static_cast<std::size_t>(step) * request.path_count;
    const double spot = paths[offset + path];
    const double intrinsic = payoff(request.payoff_sign, spot, request.strike);
    if (intrinsic <= 0.0) return;
    const double scaled = spot / request.strike;
    const double continuation = solution->coefficients[0] +
                                scaled * (solution->coefficients[1] + scaled * solution->coefficients[2]);
    if (isfinite(continuation) && intrinsic / request.strike > continuation)
        cash_flows[path] = intrinsic;
}

__global__ void simulate_accumulator_paths(
    CudaAccumulatorRequest request, const CudaSimulationStep* steps, std::size_t step_count,
    double* payoffs, int* invalid)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= request.path_count) return;

    curandStatePhilox4_32_10_t generator;
    curand_init(request.seed, static_cast<unsigned long long>(path), 0, &generator);
    double spot = request.spot;
    double quantity = request.initial_quantity;
    double discount = 1.0;
    for (std::size_t index = 0; index < step_count; ++index) {
        const auto step = steps[index];
        spot *= exp(step.drift + step.diffusion * curand_normal_double(&generator));
        if (!isfinite(spot) || spot <= 0.0) {
            atomicExch(invalid, 1);
            payoffs[path] = 0.0;
            return;
        }
        discount = step.discount;
        if (spot >= request.knock_out) break;
        quantity += spot < request.strike
                        ? request.daily_quantity * request.acceleration
                        : request.daily_quantity;
    }
    const double payoff = quantity * (spot - request.strike) * discount;
    if (!isfinite(payoff)) {
        atomicExch(invalid, 1);
        payoffs[path] = 0.0;
        return;
    }
    payoffs[path] = payoff;
}

__global__ void simulate_autocallable_paths(
    CudaAutocallableRequest request, const CudaAutocallableStep* steps, std::size_t step_count,
    double* payoffs, int* invalid)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= request.path_count) return;

    curandStatePhilox4_32_10_t generator;
    curand_init(request.seed, static_cast<unsigned long long>(path), 0, &generator);
    double spot = request.spot;
    auto state = request.initial_state;
    for (std::size_t index = 0; index < step_count; ++index) {
        const auto step = steps[index];
        spot *= exp(step.simulation.drift +
                    step.simulation.diffusion * curand_normal_double(&generator));
        if (!isfinite(spot) || spot <= 0.0) {
            atomicExch(invalid, 1);
            payoffs[path] = 0.0;
            return;
        }
        state.knocked_in = program_knocked_in(
            request.program, spot, state.knocked_in, false);
        if (!step.event.active) continue;
        const double coupon = program_observation_coupon(step.event, spot);
        if (spot >= step.event.knock_out_level) {
            const double payoff =
                (request.program.principal_ratio + coupon) * step.simulation.discount +
                state.coupons;
            if (!isfinite(payoff)) atomicExch(invalid, 1);
            payoffs[path] = isfinite(payoff) ? payoff : 0.0;
            return;
        }
        if (request.program.carries_observation_coupon)
            state.coupons += coupon * step.simulation.discount;
    }
    state.knocked_in = program_knocked_in(request.program, spot, state.knocked_in, true);
    const double payoff = state.coupons +
                          request.terminal_discount * program_terminal_settlement(
                                                          request.program, spot, state.knocked_in);
    if (!isfinite(payoff)) {
        atomicExch(invalid, 1);
        payoffs[path] = 0.0;
        return;
    }
    payoffs[path] = payoff;
}

template <typename Step>
    requires std::is_trivially_copyable_v<Step>
CudaPricingResult upload_steps(DeviceMemory& memory, std::span<const Step> steps)
{
    if (steps.empty()) return {CudaPricingStatus::success, 0.0, nullptr};
    auto result = allocate(memory, steps.size_bytes());
    if (result.status != CudaPricingStatus::success) return result;
    const cudaError_t status = cudaMemcpy(
        memory.get(), steps.data(), steps.size_bytes(), cudaMemcpyHostToDevice);
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

CudaPricingResult reduce_payoff_mean(const double* payoffs, int count, double* total, MonteCarloMean* partials)
{
    const int block_count = std::min((count + threads_per_block - 1) / threads_per_block, 256);
    reduce_payoffs<<<block_count, threads_per_block>>>(payoffs, count, partials);
    auto status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    reduce_payoffs<<<1, threads_per_block>>>(static_cast<const MonteCarloMean*>(partials), block_count, total);
    status = cudaGetLastError();
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

CudaPricingResult finish_path_simulation(const PathOutputs& outputs, int path_count)
{
    const auto reduction = reduce_payoff_mean(
        outputs.payoffs, path_count, &outputs.result->mean, outputs.partials);
    if (reduction.status != CudaPricingStatus::success) return reduction;

    PathSimulationResult result{};
    const auto status = cudaMemcpy(&result, outputs.result, sizeof(result), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) return error_result(status);
    if (result.invalid != 0)
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo simulation produced a non-finite path"};

    if (!std::isfinite(result.mean))
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo reduction produced a non-finite result"};
    return {CudaPricingStatus::success, result.mean, nullptr};
}

CudaPricingResult allocate_path_outputs(int path_count, PathOutputs& outputs)
{
    static_assert(sizeof(double) % alignof(MonteCarloMean) == 0);
    static_assert(sizeof(MonteCarloMean) % alignof(PathSimulationResult) == 0);
    const int block_count = std::min((path_count + threads_per_block - 1) / threads_per_block, 256);
    const auto payoff_bytes = static_cast<std::size_t>(path_count) * sizeof(double);
    const auto partial_bytes = static_cast<std::size_t>(block_count) * sizeof(MonteCarloMean);
    const auto result = allocate(outputs.memory, payoff_bytes + partial_bytes + sizeof(PathSimulationResult));
    if (result.status != CudaPricingStatus::success) return result;
    auto* storage = static_cast<std::byte*>(outputs.memory.get());
    outputs.payoffs = reinterpret_cast<double*>(storage);
    outputs.partials = reinterpret_cast<MonteCarloMean*>(storage + payoff_bytes);
    outputs.result = reinterpret_cast<PathSimulationResult*>(storage + payoff_bytes + partial_bytes);
    const cudaError_t status = cudaMemset(outputs.result, 0, sizeof(PathSimulationResult));
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

} // namespace

CudaPricingResult cuda_european_price(CudaEuropeanRequest request)
{
    const auto device = check_device_available();
    if (device.status != CudaPricingStatus::success) return device;

    const int pair_count = request.path_count / 2;
    PathOutputs outputs;
    const auto allocation = allocate_path_outputs(request.path_count, outputs);
    if (allocation.status != CudaPricingStatus::success) return allocation;

    const int block_count = (pair_count + threads_per_block - 1) / threads_per_block;
    simulate_payoff_pairs<<<block_count, threads_per_block>>>(
        request, pair_count, outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    return finish_path_simulation(outputs, request.path_count);
}

CudaPricingResult cuda_american_price(CudaAmericanRequest request)
{
    const auto device = check_device_available();
    if (device.status != CudaPricingStatus::success) return device;

    const int pair_count = request.path_count / 2;
    const int pair_block_count =
        (pair_count + threads_per_block - 1) / threads_per_block;
    const int path_block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    DeviceMemory paths;
    DeviceMemory cash_flows;
    DeviceMemory invalid;
    DeviceMemory partials;
    DeviceMemory summary;
    auto allocation = allocate(
        paths, static_cast<std::size_t>(request.path_count) *
                   static_cast<std::size_t>(request.step_count) * sizeof(double));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(
        cash_flows, static_cast<std::size_t>(request.path_count) * sizeof(double));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(invalid, sizeof(int));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(
        partials, static_cast<std::size_t>(path_block_count) *
                      sizeof(RegressionStatistics));
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(summary, sizeof(RegressionSolution));
    if (allocation.status != CudaPricingStatus::success) return allocation;

    cudaError_t status = cudaMemset(invalid.get(), 0, sizeof(int));
    if (status != cudaSuccess) return error_result(status);
    simulate_american_pairs<<<pair_block_count, threads_per_block>>>(
        request, pair_count, static_cast<double*>(paths.get()),
        static_cast<int*>(invalid.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    int invalid_result = 0;
    status = cudaMemcpy(&invalid_result, invalid.get(), sizeof(int), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) return error_result(status);
    if (invalid_result != 0)
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo simulation produced a non-finite path"};

    initialize_cash_flows<<<path_block_count, threads_per_block>>>(
        request, static_cast<const double*>(paths.get()),
        static_cast<double*>(cash_flows.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);

    for (int step = request.step_count - 2; step >= 1; --step) {
        discount_and_collect_statistics<<<path_block_count, threads_per_block>>>(
            request, step, static_cast<const double*>(paths.get()),
            static_cast<double*>(cash_flows.get()),
            static_cast<RegressionStatistics*>(partials.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);
        reduce_statistics<<<1, threads_per_block>>>(
            static_cast<const RegressionStatistics*>(partials.get()), path_block_count,
            static_cast<RegressionSolution*>(summary.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);

        apply_exercise<<<path_block_count, threads_per_block>>>(
            request, step, static_cast<const double*>(paths.get()),
            static_cast<double*>(cash_flows.get()), static_cast<const RegressionSolution*>(summary.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);
    }

    const auto reduction = reduce_payoff_mean(
        static_cast<const double*>(cash_flows.get()), request.path_count,
        static_cast<double*>(summary.get()), static_cast<MonteCarloMean*>(partials.get()));
    if (reduction.status != CudaPricingStatus::success) return reduction;
    double payoff_mean = 0.0;
    status = cudaMemcpy(&payoff_mean, summary.get(), sizeof(double), cudaMemcpyDeviceToHost);
    if (status != cudaSuccess) return error_result(status);
    if (!std::isfinite(payoff_mean))
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo reduction produced a non-finite result"};
    return {CudaPricingStatus::success, payoff_mean, nullptr};
}

CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const CudaSimulationStep> steps)
{
    const auto device = check_device_available();
    if (device.status != CudaPricingStatus::success) return device;

    DeviceMemory device_steps;
    auto result = upload_steps(device_steps, steps);
    if (result.status != CudaPricingStatus::success) return result;
    PathOutputs outputs;
    result = allocate_path_outputs(request.path_count, outputs);
    if (result.status != CudaPricingStatus::success) return result;

    const int block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    simulate_accumulator_paths<<<block_count, threads_per_block>>>(
        request, static_cast<const CudaSimulationStep*>(device_steps.get()), steps.size(),
        outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    return finish_path_simulation(outputs, request.path_count);
}

CudaPricingResult cuda_autocallable_price(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps)
{
    const auto device = check_device_available();
    if (device.status != CudaPricingStatus::success) return device;

    DeviceMemory device_steps;
    auto result = upload_steps(device_steps, steps);
    if (result.status != CudaPricingStatus::success) return result;
    PathOutputs outputs;
    result = allocate_path_outputs(request.path_count, outputs);
    if (result.status != CudaPricingStatus::success) return result;

    const int block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    simulate_autocallable_paths<<<block_count, threads_per_block>>>(
        request, static_cast<const CudaAutocallableStep*>(device_steps.get()), steps.size(),
        outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    return finish_path_simulation(outputs, request.path_count);
}

} // namespace kiyosi::detail
