#include "monte_carlo_cuda.hpp"
#include "monte_carlo_mean.hpp"
#include "monte_carlo_cuda_reduction.hpp"
#include "vanilla/monte_carlo_regression.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <memory>
#include <limits>
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

struct StreamDeleter {
    void operator()(cudaStream_t stream) const noexcept
    {
        // Failed requests may still be reading caller-owned host inputs.
        cudaStreamSynchronize(stream);
        cudaStreamDestroy(stream);
    }
};

using CudaStream = std::unique_ptr<std::remove_pointer_t<cudaStream_t>, StreamDeleter>;

struct RequestCompletion {
    cudaStream_t stream;
    // Caller-owned step arrays must remain alive even when a queued operation fails.
    ~RequestCompletion() { cudaStreamSynchronize(stream); }
};

struct DeviceDeleter {
    cudaStream_t stream{};

    void operator()(void* pointer) const noexcept
    {
        if (pointer != nullptr) cudaFreeAsync(pointer, stream);
    }
};

struct DeviceMemory {
    std::unique_ptr<void, DeviceDeleter> pointer;
    std::size_t capacity{};
    void* get() const noexcept { return pointer.get(); }
};

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

CudaPricingResult create_stream(CudaStream& stream)
{
    cudaStream_t handle = nullptr;
    const cudaError_t status = cudaStreamCreateWithFlags(&handle, cudaStreamNonBlocking);
    if (status != cudaSuccess) return error_result(status);
    stream.reset(handle);
    return {CudaPricingStatus::success, 0.0, nullptr};
}

CudaPricingResult allocate(DeviceMemory& memory, std::size_t size, cudaStream_t stream)
{
    if (memory.capacity >= size) return {CudaPricingStatus::success, 0.0, nullptr};
    void* pointer = nullptr;
    const cudaError_t status = cudaMallocAsync(&pointer, size, stream);
    if (status != cudaSuccess) return error_result(status);
    memory.pointer = std::unique_ptr<void, DeviceDeleter>{pointer, DeviceDeleter{stream}};
    memory.capacity = size;
    return {CudaPricingStatus::success, 0.0, nullptr};
}

CudaPricingResult download(void* host, const void* device, std::size_t size, cudaStream_t stream)
{
    auto status = cudaMemcpyAsync(host, device, size, cudaMemcpyDeviceToHost, stream);
    if (status == cudaSuccess) status = cudaStreamSynchronize(stream);
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
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
    int device = 0;
    auto capability = cudaGetDevice(&device);
    if (capability != cudaSuccess) return error_result(capability);
    int memory_pools = 0;
    capability = cudaDeviceGetAttribute(&memory_pools, cudaDevAttrMemoryPoolsSupported, device);
    if (capability != cudaSuccess) return error_result(capability);
    if (!memory_pools)
        return {CudaPricingStatus::unavailable, 0.0, "CUDA device does not support stream-ordered memory allocation"};
    return {CudaPricingStatus::success, 0.0, nullptr};
}

__device__ double payoff(int sign, double spot, double strike)
{
    return fmax(static_cast<double>(sign) * (spot - strike), 0.0);
}

__global__ void generate_european_normals(std::uint64_t seed, int pair_count, double* normals)
{
    const int pair = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (pair >= pair_count) return;

    curandStatePhilox4_32_10_t generator;
    curand_init(seed, static_cast<unsigned long long>(pair), 0, &generator);
    normals[pair] = curand_normal_double(&generator);
}

__global__ void simulate_payoff_pairs(CudaEuropeanRequest request, int pair_count,
                                      const double* normals, double2* growth, bool rebuild,
                                      double* payoffs, int* invalid)
{
    const int pair = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (pair >= pair_count) return;
    double2 factors;
    if (!normals || rebuild) {
        double normal;
        if (normals) normal = normals[pair];
        else {
            curandStatePhilox4_32_10_t generator;
            curand_init(request.seed, static_cast<unsigned long long>(pair), 0, &generator);
            normal = curand_normal_double(&generator);
        }
        factors = {exp(request.drift + request.diffusion * normal),
                   exp(request.drift - request.diffusion * normal)};
        if (growth) growth[pair] = factors;
    } else {
        factors = growth[pair];
    }
    const double positive_spot = request.spot * factors.x;
    const double negative_spot = request.spot * factors.y;
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
    MonteCarloDiscount discount;
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
    const double payoff = discount.apply(quantity * (spot - request.strike));
    if (!isfinite(payoff)) {
        atomicExch(invalid, 1);
        payoffs[path] = 0.0;
        return;
    }
    payoffs[path] = payoff;
}

template <bool Paired>
__global__ void simulate_autocallable_paths(
    CudaAutocallableRequest request, const CudaAutocallableStep* steps, std::size_t step_count,
    double* payoffs, double* alternate_payoffs, int* invalid)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= request.path_count) return;

    curandStatePhilox4_32_10_t generator;
    curand_init(request.seed, static_cast<unsigned long long>(path), 0, &generator);
    double spot = request.spot;
    auto state = request.initial_state;
    auto alternate_state = request.alternate_initial_state;
    double payoff = 0.0;
    double alternate_payoff = 0.0;
    bool knocked_out = false;
    for (std::size_t index = 0; index < step_count; ++index) {
        const auto step = steps[index];
        spot *= exp(step.simulation.drift +
                    step.simulation.diffusion * curand_normal_double(&generator));
        if (!isfinite(spot) || spot <= 0.0) {
            atomicExch(invalid, 1);
            payoffs[path] = 0.0;
            if constexpr (Paired) alternate_payoffs[path] = 0.0;
            return;
        }
        state.knocked_in = program_knocked_in(request.program, spot, state.knocked_in, false);
        if (!step.event.active) continue;
        const double coupon = program_observation_coupon(step.event, spot);
        double alternate_coupon = 0.0;
        if constexpr (Paired) {
            auto event = step.event;
            event.coupon = step.alternate_coupon;
            alternate_coupon = program_observation_coupon(event, spot);
        }
        if (spot >= step.event.knock_out_level) {
            payoff = step.simulation.discount.apply(request.program.principal_ratio + coupon) + state.coupons;
            if constexpr (Paired)
                alternate_payoff = step.simulation.discount.apply(request.alternate_program.principal_ratio + alternate_coupon) + alternate_state.coupons;
            knocked_out = true;
            break;
        }
        if (request.program.carries_observation_coupon)
            state.coupons += step.simulation.discount.apply(coupon);
        if constexpr (Paired)
            if (request.alternate_program.carries_observation_coupon)
                alternate_state.coupons += step.simulation.discount.apply(alternate_coupon);
    }
    if (!knocked_out) {
        state.knocked_in = program_knocked_in(request.program, spot, state.knocked_in, true);
        payoff = state.coupons + request.terminal_discount.apply(program_terminal_settlement(request.program, spot, state.knocked_in));
        if constexpr (Paired)
            alternate_payoff = alternate_state.coupons + request.terminal_discount.apply(program_terminal_settlement(request.alternate_program, spot, state.knocked_in));
    }
    if (!isfinite(payoff)) atomicExch(invalid, 1);
    payoffs[path] = isfinite(payoff) ? payoff : 0.0;
    if constexpr (Paired) {
        if (!isfinite(alternate_payoff)) atomicExch(invalid, 1);
        alternate_payoffs[path] = isfinite(alternate_payoff) ? alternate_payoff : 0.0;
    }
}

template <typename Request>
struct SpotRequests {
    // Kernel parameters and per-thread states cannot use host-only std::array accessors.
    Request values[5];  // NOLINT(modernize-avoid-c-arrays)
    double* payoffs[5]; // NOLINT(modernize-avoid-c-arrays)
    int count;
    int* invalid;
};

template <typename Request>
__device__ void write_spot_payoff(const SpotRequests<Request>& requests, int bump, int path, double value)
{
    if (!isfinite(value)) atomicExch(requests.invalid, 1);
    requests.payoffs[bump][path] = isfinite(value) ? value : 0.0;
}

__global__ void simulate_accumulator_spot_paths(
    SpotRequests<CudaAccumulatorRequest> requests, const CudaSimulationStep* steps, std::size_t step_count)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= requests.values[0].path_count) return;
    curandStatePhilox4_32_10_t generator;
    curand_init(requests.values[0].seed, static_cast<unsigned long long>(path), 0, &generator);
    double spots[5], quantities[5]; // NOLINT(modernize-avoid-c-arrays)
    bool done[5]{};                 // NOLINT(modernize-avoid-c-arrays)
    int remaining = requests.count;
    for (int bump = 0; bump < requests.count; ++bump) {
        spots[bump] = requests.values[bump].spot;
        quantities[bump] = requests.values[bump].initial_quantity;
    }
    MonteCarloDiscount discount;
    for (std::size_t index = 0; index < step_count; ++index) {
        const auto step = steps[index];
        const double growth = exp(step.drift + step.diffusion * curand_normal_double(&generator));
        discount = step.discount;
        for (int bump = 0; bump < requests.count; ++bump) {
            if (done[bump]) continue;
            const auto request = requests.values[bump];
            spots[bump] *= growth;
            if (!isfinite(spots[bump]) || spots[bump] <= 0.0) {
                atomicExch(requests.invalid, 1);
                for (int output = 0; output < requests.count; ++output)
                    requests.payoffs[output][path] = 0.0;
                return;
            }
            if (spots[bump] >= request.knock_out) {
                write_spot_payoff(requests, bump, path, discount.apply(quantities[bump] * (spots[bump] - request.strike)));
                done[bump] = true;
                --remaining;
            } else {
                quantities[bump] += spots[bump] < request.strike
                                        ? request.daily_quantity * request.acceleration
                                        : request.daily_quantity;
            }
        }
        if (remaining == 0) break;
    }
    for (int bump = 0; bump < requests.count; ++bump)
        if (!done[bump])
            write_spot_payoff(requests, bump, path, discount.apply(quantities[bump] * (spots[bump] - requests.values[bump].strike)));
}

__global__ void simulate_autocallable_spot_paths(
    SpotRequests<CudaAutocallableRequest> requests, const CudaAutocallableStep* steps, std::size_t step_count)
{
    const int path = static_cast<int>(blockIdx.x * blockDim.x + threadIdx.x);
    if (path >= requests.values[0].path_count) return;
    curandStatePhilox4_32_10_t generator;
    curand_init(requests.values[0].seed, static_cast<unsigned long long>(path), 0, &generator);
    double spots[5];                 // NOLINT(modernize-avoid-c-arrays)
    AutocallablePathState states[5]; // NOLINT(modernize-avoid-c-arrays)
    bool done[5]{};                  // NOLINT(modernize-avoid-c-arrays)
    int remaining = requests.count;
    for (int bump = 0; bump < requests.count; ++bump) {
        spots[bump] = requests.values[bump].spot;
        states[bump] = requests.values[bump].initial_state;
    }
    for (std::size_t index = 0; index < step_count; ++index) {
        const auto step = steps[index];
        const double growth = exp(step.simulation.drift + step.simulation.diffusion * curand_normal_double(&generator));
        for (int bump = 0; bump < requests.count; ++bump) {
            if (done[bump]) continue;
            const auto& program = requests.values[bump].program;
            auto& state = states[bump];
            spots[bump] *= growth;
            if (!isfinite(spots[bump]) || spots[bump] <= 0.0) {
                atomicExch(requests.invalid, 1);
                for (int output = 0; output < requests.count; ++output)
                    requests.payoffs[output][path] = 0.0;
                return;
            }
            state.knocked_in = program_knocked_in(program, spots[bump], state.knocked_in, false);
            if (!step.event.active) continue;
            const double coupon = program_observation_coupon(step.event, spots[bump]);
            if (spots[bump] >= step.event.knock_out_level) {
                write_spot_payoff(requests, bump, path, step.simulation.discount.apply(program.principal_ratio + coupon) + state.coupons);
                done[bump] = true;
                --remaining;
            } else if (program.carries_observation_coupon) {
                state.coupons += step.simulation.discount.apply(coupon);
            }
        }
        if (remaining == 0) break;
    }
    for (int bump = 0; bump < requests.count; ++bump) {
        if (done[bump]) continue;
        const auto request = requests.values[bump];
        auto state = states[bump];
        state.knocked_in = program_knocked_in(request.program, spots[bump], state.knocked_in, true);
        write_spot_payoff(requests, bump, path, state.coupons + request.terminal_discount.apply(program_terminal_settlement(request.program, spots[bump], state.knocked_in)));
    }
}

template <typename Step>
    requires std::is_trivially_copyable_v<Step>
CudaPricingResult upload_steps(DeviceMemory& memory, std::span<const Step> steps, cudaStream_t stream)
{
    if (steps.empty()) return {CudaPricingStatus::success, 0.0, nullptr};
    auto result = allocate(memory, steps.size_bytes(), stream);
    if (result.status != CudaPricingStatus::success) return result;
    const cudaError_t status = cudaMemcpyAsync(
        memory.get(), steps.data(), steps.size_bytes(), cudaMemcpyHostToDevice, stream);
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

CudaPricingResult reduce_payoff_mean(const double* payoffs, int count, double* total, MonteCarloMean* partials, cudaStream_t stream)
{
    const int block_count = std::min((count + threads_per_block - 1) / threads_per_block, 256);
    reduce_payoffs<<<block_count, threads_per_block, 0, stream>>>(payoffs, count, partials);
    auto status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    reduce_payoffs<<<1, threads_per_block, 0, stream>>>(static_cast<const MonteCarloMean*>(partials), block_count, total);
    status = cudaGetLastError();
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

CudaPricingResult finish_path_simulation(const PathOutputs& outputs, int path_count, cudaStream_t stream)
{
    const auto reduction = reduce_payoff_mean(
        outputs.payoffs, path_count, &outputs.result->mean, outputs.partials, stream);
    if (reduction.status != CudaPricingStatus::success) return reduction;

    PathSimulationResult result{};
    const auto transfer = download(&result, outputs.result, sizeof(result), stream);
    if (transfer.status != CudaPricingStatus::success) return transfer;
    if (result.invalid != 0)
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo simulation produced a non-finite path"};

    if (!std::isfinite(result.mean))
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo reduction produced a non-finite result"};
    return {CudaPricingStatus::success, result.mean, nullptr};
}

CudaPricingResult allocate_path_outputs(int path_count, PathOutputs& outputs, cudaStream_t stream)
{
    static_assert(sizeof(double) % alignof(MonteCarloMean) == 0);
    static_assert(sizeof(MonteCarloMean) % alignof(PathSimulationResult) == 0);
    const int block_count = std::min((path_count + threads_per_block - 1) / threads_per_block, 256);
    const auto payoff_bytes = static_cast<std::size_t>(path_count) * sizeof(double);
    const auto partial_bytes = static_cast<std::size_t>(block_count) * sizeof(MonteCarloMean);
    const auto result = allocate(outputs.memory, payoff_bytes + partial_bytes + sizeof(PathSimulationResult), stream);
    if (result.status != CudaPricingStatus::success) return result;
    auto* storage = static_cast<std::byte*>(outputs.memory.get());
    outputs.payoffs = reinterpret_cast<double*>(storage);
    outputs.partials = reinterpret_cast<MonteCarloMean*>(storage + payoff_bytes);
    outputs.result = reinterpret_cast<PathSimulationResult*>(storage + payoff_bytes + partial_bytes);
    const cudaError_t status = cudaMemsetAsync(outputs.result, 0, sizeof(PathSimulationResult), stream);
    return status == cudaSuccess ? CudaPricingResult{CudaPricingStatus::success, 0.0, nullptr}
                                 : error_result(status);
}

} // namespace

struct CudaWorkspace::Impl {
    CudaStream owned_stream;
    DeviceMemory steps, paths, cash_flows, invalid, partials, summary, normals, growth;
    std::array<PathOutputs, 5> outputs;
    int pair_count{};
    std::uint64_t seed{};
    double drift = std::numeric_limits<double>::quiet_NaN();
    double diffusion = std::numeric_limits<double>::quiet_NaN();

    CudaPricingResult prepare()
    {
        if (owned_stream) return {CudaPricingStatus::success, 0.0, nullptr};
        const auto device = check_device_available();
        if (device.status != CudaPricingStatus::success) return device;
        return create_stream(owned_stream);
    }
};

CudaWorkspace::CudaWorkspace() = default;
CudaWorkspace::~CudaWorkspace() = default;

CudaWorkspace::Impl& CudaWorkspace::storage()
{
    if (!impl_) impl_ = std::make_unique<Impl>();
    return *impl_;
}

namespace {
template <typename Request, typename Step, typename Launch>
CudaSpotPricingResult spot_prices(std::span<const Request> requests, std::span<const Step> steps,
                                  CudaWorkspace* workspace, const Launch& launch)
{
    CudaWorkspace local;
    auto& storage = (workspace ? *workspace : local).storage();
    const auto created = storage.prepare();
    if (created.status != CudaPricingStatus::success) return {created};
    const auto stream = storage.owned_stream.get();
    const RequestCompletion completion{stream};
    auto result = upload_steps(storage.steps, steps, stream);
    if (result.status != CudaPricingStatus::success) return {result};
    SpotRequests<Request> batch{};
    batch.count = static_cast<int>(requests.size());
    for (int bump = 0; bump < batch.count; ++bump) {
        batch.values[bump] = requests[bump];
        auto& outputs = storage.outputs[bump];
        result = allocate_path_outputs(requests[bump].path_count, outputs, stream);
        if (result.status != CudaPricingStatus::success) return {result};
        batch.payoffs[bump] = outputs.payoffs;
    }
    batch.invalid = &storage.outputs[0].result->invalid;
    const int path_count = requests[0].path_count;
    launch(batch, static_cast<const Step*>(storage.steps.get()), steps.size(),
           (path_count + threads_per_block - 1) / threads_per_block, stream);
    const auto status = cudaGetLastError();
    if (status != cudaSuccess) return {error_result(status)};
    CudaSpotPricingResult prices{};
    for (int bump = 0; bump < batch.count; ++bump) {
        prices.result = finish_path_simulation(storage.outputs[bump], path_count, stream);
        if (prices.result.status != CudaPricingStatus::success) return prices;
        prices.payoff_means[bump] = prices.result.payoff_mean;
    }
    return prices;
}
} // namespace

CudaSpotPricingResult cuda_accumulator_spot_prices(
    std::span<const CudaAccumulatorRequest> requests, std::span<const CudaSimulationStep> steps, CudaWorkspace* workspace)
{
    return spot_prices(requests, steps, workspace, [](const auto& batch, auto* device_steps, auto step_count, int blocks, cudaStream_t stream) {
        simulate_accumulator_spot_paths<<<blocks, threads_per_block, 0, stream>>>(batch, device_steps, step_count);
    });
}

CudaSpotPricingResult cuda_autocallable_spot_prices(
    std::span<const CudaAutocallableRequest> requests, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace)
{
    return spot_prices(requests, steps, workspace, [](const auto& batch, auto* device_steps, auto step_count, int blocks, cudaStream_t stream) {
        simulate_autocallable_spot_paths<<<blocks, threads_per_block, 0, stream>>>(batch, device_steps, step_count);
    });
}

CudaPricingResult cuda_european_price(CudaEuropeanRequest request, CudaWorkspace* workspace)
{
    CudaWorkspace local;
    auto& storage = (workspace ? *workspace : local).storage();
    const auto created = storage.prepare();
    if (created.status != CudaPricingStatus::success) return created;
    const auto stream = storage.owned_stream.get();
    const RequestCompletion completion{stream};

    const int pair_count = request.path_count / 2;
    const bool new_samples = storage.pair_count != pair_count || storage.seed != request.seed;
    if (new_samples) storage.pair_count = 0;
    auto& outputs = storage.outputs[0];
    auto allocation = allocate_path_outputs(request.path_count, outputs, stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;
    if (workspace) {
        allocation = allocate(storage.normals, static_cast<std::size_t>(pair_count) * sizeof(double), stream);
        if (allocation.status != CudaPricingStatus::success) return allocation;
        allocation = allocate(storage.growth, static_cast<std::size_t>(pair_count) * sizeof(double2), stream);
        if (allocation.status != CudaPricingStatus::success) return allocation;
    }

    const int block_count = (pair_count + threads_per_block - 1) / threads_per_block;
    if (workspace && new_samples) {
        generate_european_normals<<<block_count, threads_per_block, 0, stream>>>(request.seed, pair_count, static_cast<double*>(storage.normals.get()));
        const auto status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);
    }
    const bool rebuild = new_samples || storage.drift != request.drift || storage.diffusion != request.diffusion;
    if (rebuild) storage.pair_count = 0;
    simulate_payoff_pairs<<<block_count, threads_per_block, 0, stream>>>(
        request, pair_count, static_cast<const double*>(storage.normals.get()), static_cast<double2*>(storage.growth.get()), rebuild,
        outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    const auto result = finish_path_simulation(outputs, request.path_count, stream);
    if (result.status == CudaPricingStatus::success) {
        storage.pair_count = pair_count;
        storage.seed = request.seed;
        storage.drift = request.drift;
        storage.diffusion = request.diffusion;
    } else {
        storage.pair_count = 0;
    }
    return result;
}

CudaPricingResult cuda_american_price(CudaAmericanRequest request, CudaWorkspace* workspace)
{
    CudaWorkspace local;
    auto& storage = (workspace ? *workspace : local).storage();
    const auto created = storage.prepare();
    if (created.status != CudaPricingStatus::success) return created;
    const auto stream = storage.owned_stream.get();
    const RequestCompletion completion{stream};

    const int pair_count = request.path_count / 2;
    const int pair_block_count =
        (pair_count + threads_per_block - 1) / threads_per_block;
    const int path_block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    auto& paths = storage.paths;
    auto& cash_flows = storage.cash_flows;
    auto& invalid = storage.invalid;
    auto& partials = storage.partials;
    auto& summary = storage.summary;
    auto allocation = allocate(
        paths, static_cast<std::size_t>(request.path_count) * static_cast<std::size_t>(request.step_count) * sizeof(double), stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(
        cash_flows, static_cast<std::size_t>(request.path_count) * sizeof(double), stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(invalid, sizeof(int), stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(
        partials, static_cast<std::size_t>(path_block_count) * sizeof(RegressionStatistics), stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;
    allocation = allocate(summary, sizeof(RegressionSolution), stream);
    if (allocation.status != CudaPricingStatus::success) return allocation;

    cudaError_t status = cudaMemsetAsync(invalid.get(), 0, sizeof(int), stream);
    if (status != cudaSuccess) return error_result(status);
    simulate_american_pairs<<<pair_block_count, threads_per_block, 0, stream>>>(
        request, pair_count, static_cast<double*>(paths.get()),
        static_cast<int*>(invalid.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    int invalid_result = 0;
    const auto invalid_transfer = download(&invalid_result, invalid.get(), sizeof(int), stream);
    if (invalid_transfer.status != CudaPricingStatus::success) return invalid_transfer;
    if (invalid_result != 0)
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo simulation produced a non-finite path"};

    initialize_cash_flows<<<path_block_count, threads_per_block, 0, stream>>>(
        request, static_cast<const double*>(paths.get()),
        static_cast<double*>(cash_flows.get()));
    status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);

    for (int step = request.step_count - 2; step >= 1; --step) {
        discount_and_collect_statistics<<<path_block_count, threads_per_block, 0, stream>>>(
            request, step, static_cast<const double*>(paths.get()),
            static_cast<double*>(cash_flows.get()),
            static_cast<RegressionStatistics*>(partials.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);
        reduce_statistics<<<1, threads_per_block, 0, stream>>>(
            static_cast<const RegressionStatistics*>(partials.get()), path_block_count,
            static_cast<RegressionSolution*>(summary.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);

        apply_exercise<<<path_block_count, threads_per_block, 0, stream>>>(
            request, step, static_cast<const double*>(paths.get()),
            static_cast<double*>(cash_flows.get()), static_cast<const RegressionSolution*>(summary.get()));
        status = cudaGetLastError();
        if (status != cudaSuccess) return error_result(status);
    }

    const auto reduction = reduce_payoff_mean(
        static_cast<const double*>(cash_flows.get()), request.path_count,
        static_cast<double*>(summary.get()), static_cast<MonteCarloMean*>(partials.get()), stream);
    if (reduction.status != CudaPricingStatus::success) return reduction;
    double payoff_mean = 0.0;
    const auto transfer = download(&payoff_mean, summary.get(), sizeof(double), stream);
    if (transfer.status != CudaPricingStatus::success) return transfer;
    if (!std::isfinite(payoff_mean))
        return {CudaPricingStatus::invalid_result, 0.0,
                "CUDA Monte Carlo reduction produced a non-finite result"};
    return {CudaPricingStatus::success, payoff_mean, nullptr};
}

CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const CudaSimulationStep> steps, CudaWorkspace* workspace)
{
    CudaWorkspace local;
    auto& storage = (workspace ? *workspace : local).storage();
    const auto created = storage.prepare();
    if (created.status != CudaPricingStatus::success) return created;
    const auto stream = storage.owned_stream.get();
    const RequestCompletion completion{stream};

    auto& device_steps = storage.steps;
    auto result = upload_steps(device_steps, steps, stream);
    if (result.status != CudaPricingStatus::success) return result;
    auto& outputs = storage.outputs[0];
    result = allocate_path_outputs(request.path_count, outputs, stream);
    if (result.status != CudaPricingStatus::success) return result;

    const int block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    simulate_accumulator_paths<<<block_count, threads_per_block, 0, stream>>>(
        request, static_cast<const CudaSimulationStep*>(device_steps.get()), steps.size(),
        outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    return finish_path_simulation(outputs, request.path_count, stream);
}

template <bool Paired>
CudaPricingResult autocallable_prices(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace)
{
    CudaWorkspace local;
    auto& storage = (workspace ? *workspace : local).storage();
    const auto created = storage.prepare();
    if (created.status != CudaPricingStatus::success) return created;
    const auto stream = storage.owned_stream.get();
    const RequestCompletion completion{stream};

    auto& device_steps = storage.steps;
    auto result = upload_steps(device_steps, steps, stream);
    if (result.status != CudaPricingStatus::success) return result;
    auto& outputs = storage.outputs[0];
    result = allocate_path_outputs(request.path_count, outputs, stream);
    if (result.status != CudaPricingStatus::success) return result;

    auto& alternate_outputs = storage.outputs[1];
    if constexpr (Paired) {
        result = allocate_path_outputs(request.path_count, alternate_outputs, stream);
        if (result.status != CudaPricingStatus::success) return result;
    }
    const int block_count =
        (request.path_count + threads_per_block - 1) / threads_per_block;
    simulate_autocallable_paths<Paired><<<block_count, threads_per_block, 0, stream>>>(
        request, static_cast<const CudaAutocallableStep*>(device_steps.get()), steps.size(),
        outputs.payoffs, alternate_outputs.payoffs, &outputs.result->invalid);
    const cudaError_t status = cudaGetLastError();
    if (status != cudaSuccess) return error_result(status);
    result = finish_path_simulation(outputs, request.path_count, stream);
    if (result.status != CudaPricingStatus::success) return result;
    if constexpr (Paired) {
        const auto alternate = finish_path_simulation(alternate_outputs, request.path_count, stream);
        if (alternate.status != CudaPricingStatus::success) return alternate;
        result.alternate_payoff_mean = alternate.payoff_mean;
    }
    return result;
}

CudaPricingResult cuda_autocallable_price(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace)
{
    return autocallable_prices<false>(request, steps, workspace);
}

CudaPricingResult cuda_autocallable_coupon_prices(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace)
{
    return autocallable_prices<true>(request, steps, workspace);
}

} // namespace kiyosi::detail
