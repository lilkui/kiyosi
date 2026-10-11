#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <cuda_runtime.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <future>
#include <limits>
#include <memory>
#include <vector>

#include "pricing/engines/monte_carlo_cuda_reduction.hpp"
#include "pricing/engines/monte_carlo_cuda.hpp"
#include "pricing/engines/vanilla/monte_carlo_regression.hpp"

namespace {
struct DeviceDeleter {
    void operator()(void* pointer) const { cudaFree(pointer); }
};

__global__ void solve_regression(const double* input, double* output)
{
    double samples[3][4];     // NOLINT(modernize-avoid-c-arrays): exercise the shared solver with device-accessible storage.
    double coefficients[3]{}; // NOLINT(modernize-avoid-c-arrays): device code cannot call host std::array methods.
    for (int row = 0; row < 3; ++row)
        for (int column = 0; column < 4; ++column)
            samples[row][column] = input[row * 4 + column];
    output[3] = kiyosi::detail::solve_quadratic(samples, coefficients) ? 1.0 : 0.0;
    for (int index = 0; index < 3; ++index)
        output[index] = coefficients[index];
}
} // namespace

TEST_CASE("CUDA pricing completes independently of unrelated streams", "[cuda][monte-carlo-performance]")
{
    using namespace kiyosi::detail;
    const auto price = [] {
        CudaWorkspace workspace;
        const std::array<CudaSimulationStep, 1> accumulator_steps{{{0.0, 0.1, 0.99}}};
        const std::array<CudaAutocallableStep, 1> autocallable_steps{{{{0.0, 0.1, 0.99}, {110.0, 0.01, 0.0, false, true}}}};
        AutocallableProgram program{};
        program.principal_ratio = 1.0;
        return std::array{
            cuda_european_price({512, 42, 100.0, 100.0, 0.01, 0.2, 1}, &workspace),
            cuda_american_price({512, 3, 42, 100.0, 100.0, 0.01, 0.1, 0.99, -1}, &workspace),
            cuda_accumulator_price({512, 42, 100.0, 90.0, 120.0, 1.0, 2.0, 0.0}, accumulator_steps, &workspace),
            cuda_autocallable_price({512, 42, 100.0, {0.99}, program, {}}, autocallable_steps, &workspace)};
    };
    for (const auto result : price())
        REQUIRE(result.status == CudaPricingStatus::success);

    for (const unsigned flags : {cudaStreamDefault, cudaStreamNonBlocking}) {
        CAPTURE(flags);
        cudaStream_t unrelated{};
        REQUIRE(cudaStreamCreateWithFlags(&unrelated, flags) == cudaSuccess);
        struct Gate {
            std::promise<void> entered;
            std::promise<void> release;
        } gate;
        const auto launched = cudaLaunchHostFunc(unrelated, [](void* data) {
            auto& gate = *static_cast<Gate*>(data);
            auto released = gate.release.get_future();
            gate.entered.set_value();
            released.wait(); }, &gate);
        if (launched != cudaSuccess) {
            cudaStreamDestroy(unrelated);
            FAIL("Could not enqueue the unrelated stream gate");
        }
        gate.entered.get_future().wait();
        auto pending = std::async(std::launch::async, price);
        const auto completion = pending.wait_for(std::chrono::seconds(2));
        // Release the gate before assertions so a regression cannot deadlock cleanup.
        gate.release.set_value();
        const auto synchronized = cudaStreamSynchronize(unrelated);
        const auto destroyed = cudaStreamDestroy(unrelated);
        const auto results = pending.get();
        CHECK(completion == std::future_status::ready);
        CHECK(synchronized == cudaSuccess);
        CHECK(destroyed == cudaSuccess);
        for (const auto result : results)
            CHECK(result.status == CudaPricingStatus::success);
    }
}

TEST_CASE("CUDA workspaces preserve sample changes and recover from invalid paths", "[cuda][cuda-workspace]")
{
    using namespace kiyosi::detail;
    CudaWorkspace workspace;
    for (const int count : {2, 256, 258, 65'538, 100'002, 256, 2})
        for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
            for (const double diffusion : {0.2, 0.3}) {
                CudaEuropeanRequest request{count, seed, 100.25, 100.0, 0.01, diffusion, 1};
                const auto expected = cuda_european_price(request);
                REQUIRE(expected.status == CudaPricingStatus::success);
                for (int repetition = 0; repetition < 2; ++repetition) {
                    const auto actual = cuda_european_price(request, &workspace);
                    REQUIRE(actual.status == CudaPricingStatus::success);
                    CHECK(actual.payoff_mean == expected.payoff_mean);
                }
                request.drift = 1000.0;
                CHECK(cuda_european_price(request, &workspace).status == CudaPricingStatus::invalid_result);
                request.drift = 0.01;
                const auto recovered = cuda_european_price(request, &workspace);
                REQUIRE(recovered.status == CudaPricingStatus::success);
                CHECK(recovered.payoff_mean == expected.payoff_mean);
            }
}

TEST_CASE("CUDA American growth caches preserve scalar prices and recover from failures", "[cuda][cuda-workspace]")
{
    using namespace kiyosi::detail;
    CudaWorkspace workspace;
    const auto check = [&](CudaAmericanRequest request) {
        const auto expected = cuda_american_price(request);
        REQUIRE(expected.status == CudaPricingStatus::success);
        for (int repetition = 0; repetition < 2; ++repetition) {
            const auto actual = cuda_american_price(request, &workspace, true);
            REQUIRE(actual.status == CudaPricingStatus::success);
            CHECK(actual.payoff_mean == expected.payoff_mean);
        }
    };
    for (const int count : {2, 510, 512, 514, 65'538, 512, 2}) {
        CudaAmericanRequest request{count, 3, 42, 100.25, 100.0, 0.01, 0.2, 0.99, -1};
        for (const int steps : {3, 7, 3}) {
            request.step_count = steps;
            check(request);
            for (const auto seed : {std::numeric_limits<std::uint64_t>::max(), std::uint64_t{42}}) {
                request.seed = seed;
                check(request);
            }
            for (const double drift : {-0.02, 0.01}) {
                request.drift = drift;
                check(request);
            }
            for (const double diffusion : {0.0, 0.1, 0.2}) {
                request.diffusion = diffusion;
                check(request);
            }
            for (const double spot : {92.0, 108.0, 100.25}) {
                request.spot = spot;
                check(request);
            }
            request.payoff_sign = 1;
            request.strike = 95.0;
            request.discount = 0.98;
            check(request);
            request.payoff_sign = -1;
            check(request);
        }
        auto invalid = request;
        invalid.drift = 1000.0;
        CHECK(cuda_american_price(invalid, &workspace, true).status == CudaPricingStatus::invalid_result);
        check(request);
        invalid = request;
        invalid.spot = std::numeric_limits<double>::infinity();
        CHECK(cuda_american_price(invalid, &workspace, true).status == CudaPricingStatus::invalid_result);
        check(request);
        auto uncached = request;
        uncached.step_count = 9;
        const auto scalar = cuda_american_price(uncached);
        const auto reused_buffer = cuda_american_price(uncached, &workspace);
        REQUIRE(scalar.status == CudaPricingStatus::success);
        REQUIRE(reused_buffer.status == CudaPricingStatus::success);
        CHECK(reused_buffer.payoff_mean == scalar.payoff_mean);
        check(request);
        const auto european = cuda_european_price({count, 42, 100.25, 100.0, 0.01, 0.2, 1}, &workspace);
        REQUIRE(european.status == CudaPricingStatus::success);
        check(request);
        const auto european_again = cuda_european_price({count, 42, 100.25, 100.0, 0.01, 0.2, 1}, &workspace);
        REQUIRE(european_again.status == CudaPricingStatus::success);
        CHECK(european_again.payoff_mean == european.payoff_mean);
    }
}

TEST_CASE("CUDA shared spot paths preserve independent settlements and recover from failures", "[cuda][cuda-spot-samples]")
{
    using namespace kiyosi::detail;
    CudaWorkspace workspace;
    for (const int paths : {1, 255, 256, 257})
        for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
            for (const double diffusion : {0.0, 0.1}) {
                CAPTURE(paths, seed, diffusion);
                const std::array<double, 5> spots{90.0, 100.0, 110.0, 120.0, 50.0};
                std::array<CudaSimulationStep, 3> steps{{{0.1, diffusion, {0.99}}, {0.1, diffusion, {0.98}}, {0.1, diffusion, {0.97}}}};
                std::array<CudaAutocallableStep, 3> events{};
                for (std::size_t index = 0; index < events.size(); ++index)
                    events[index] = {steps[index], {115.0, 0.01 * (index + 1), 105.0, true, true}};
                const AutocallableProgram program{1.0, 100.0, 100.0, 60.0, 100.0, 0.08, 0.0,
                                                  AutocallableTerminalKind::downside_if_knocked_in, true, true, true};
                std::array<CudaAccumulatorRequest, 5> accumulators{};
                std::array<CudaAutocallableRequest, 5> notes{};
                std::array<double, 5> expected_accumulators{}, expected_notes{};
                for (std::size_t index = 0; index < spots.size(); ++index) {
                    accumulators[index] = {paths, seed, spots[index], 100.0, 115.0, 1.0, 2.0, 2.0};
                    notes[index] = {paths, seed, spots[index], {0.97}, program, {0.0, index % 2 == 1}};
                    const auto accumulator = cuda_accumulator_price(accumulators[index], steps);
                    const auto note = cuda_autocallable_price(notes[index], events);
                    REQUIRE(accumulator.status == CudaPricingStatus::success);
                    REQUIRE(note.status == CudaPricingStatus::success);
                    expected_accumulators[index] = accumulator.payoff_mean;
                    expected_notes[index] = note.payoff_mean;
                }
                for (const std::size_t count : {5, 2, 3, 1, 4, 5}) {
                    const auto accumulator = cuda_accumulator_spot_prices(std::span{accumulators}.first(count), steps, &workspace);
                    const auto note = cuda_autocallable_spot_prices(std::span{notes}.first(count), events, &workspace);
                    REQUIRE(accumulator.result.status == CudaPricingStatus::success);
                    REQUIRE(note.result.status == CudaPricingStatus::success);
                    for (std::size_t index = 0; index < count; ++index) {
                        CHECK(accumulator.payoff_means[index] == expected_accumulators[index]);
                        CHECK(note.payoff_means[index] == expected_notes[index]);
                    }
                }
                accumulators.back().spot = std::numeric_limits<double>::infinity();
                notes.back().spot = std::numeric_limits<double>::infinity();
                CHECK(cuda_accumulator_spot_prices(accumulators, steps, &workspace).result.status == CudaPricingStatus::invalid_result);
                CHECK(cuda_autocallable_spot_prices(notes, events, &workspace).result.status == CudaPricingStatus::invalid_result);
                accumulators.back().spot = spots.back();
                notes.back().spot = spots.back();
                const auto accumulator = cuda_accumulator_spot_prices(accumulators, steps, &workspace);
                const auto note = cuda_autocallable_spot_prices(notes, events, &workspace);
                REQUIRE(accumulator.result.status == CudaPricingStatus::success);
                REQUIRE(note.result.status == CudaPricingStatus::success);
                CHECK(accumulator.payoff_means == expected_accumulators);
                CHECK(note.payoff_means == expected_notes);
            }
}

TEST_CASE("CUDA regression solves quadratic and reduced bases", "[cuda]")
{
    void* input_pointer = nullptr;
    REQUIRE(cudaMalloc(&input_pointer, 12 * sizeof(double)) == cudaSuccess);
    const std::unique_ptr<void, DeviceDeleter> input{input_pointer};
    void* output_pointer = nullptr;
    REQUIRE(cudaMalloc(&output_pointer, 4 * sizeof(double)) == cudaSuccess);
    const std::unique_ptr<void, DeviceDeleter> output{output_pointer};
    const auto check = [&](const std::array<double, 12>& samples, const std::array<double, 3>& expected, bool valid) {
        REQUIRE(cudaMemcpy(input.get(), samples.data(), sizeof(samples), cudaMemcpyHostToDevice) == cudaSuccess);
        solve_regression<<<1, 1>>>(static_cast<const double*>(input.get()), static_cast<double*>(output.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        std::array<double, 4> actual{};
        REQUIRE(cudaMemcpy(actual.data(), output.get(), sizeof(actual), cudaMemcpyDeviceToHost) == cudaSuccess);
        CHECK(actual[3] == (valid ? 1.0 : 0.0));
        if (valid)
            for (int index = 0; index < 3; ++index)
                CHECK(actual[index] == Catch::Approx(expected[index]).margin(1e-12));
    };
    check({3, 0, 2, 14, 0, 2, 0, 6, 2, 0, 2, 12}, {2, 3, 4}, true);
    check({2, 1, 1, 7, 1, 1, 1, 5, 1, 1, 1, 5}, {2, 3, 0}, true);
    check({4, 4, 4, 20, 4, 4, 4, 20, 4, 4, 4, 20}, {5, 0, 0}, true);
    check({}, {}, false);
    std::array<double, 12> non_finite{};
    non_finite[0] = std::numeric_limits<double>::quiet_NaN();
    check(non_finite, {}, false);
}

TEST_CASE("CUDA payoff reduction preserves weights and extreme values across blocks", "[cuda][monte-carlo-performance]")
{
    using namespace kiyosi::detail;
    const auto allocate = [](std::size_t bytes) {
        void* pointer = nullptr;
        REQUIRE(cudaMalloc(&pointer, bytes) == cudaSuccess);
        return std::unique_ptr<void, DeviceDeleter>{pointer};
    };
    const auto input = allocate(100'003 * sizeof(double));
    const auto partials = allocate(256 * sizeof(MonteCarloMean));
    const auto output = allocate(sizeof(double));
    const auto reduce = [&](const std::vector<double>& values) {
        const int count = static_cast<int>(values.size());
        const int blocks = std::min((count + monte_carlo_reduction_threads - 1) / monte_carlo_reduction_threads, 256);
        REQUIRE(cudaMemcpy(input.get(), values.data(), values.size() * sizeof(double), cudaMemcpyHostToDevice) == cudaSuccess);
        reduce_payoffs<<<blocks, monte_carlo_reduction_threads>>>(static_cast<const double*>(input.get()), count,
                                                                  static_cast<MonteCarloMean*>(partials.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        reduce_payoffs<<<1, monte_carlo_reduction_threads>>>(static_cast<const MonteCarloMean*>(partials.get()), blocks,
                                                             static_cast<double*>(output.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        double result = 0.0;
        REQUIRE(cudaMemcpy(&result, output.get(), sizeof(double), cudaMemcpyDeviceToHost) == cudaSuccess);
        return result;
    };
    for (const int count : {1, 255, 256, 257, 65'535, 65'536, 65'537, 100'003}) {
        CAPTURE(count);
        std::vector<double> values(static_cast<std::size_t>(count));
        double sum = 0.0;
        for (int index = 0; index < count; ++index) {
            values[static_cast<std::size_t>(index)] = index % 13 - 6.0;
            sum += values[static_cast<std::size_t>(index)];
        }
        CHECK(reduce(values) == Catch::Approx(sum / count).margin(1e-14));
    }
    const double maximum = std::numeric_limits<double>::max();
    for (const double value : {maximum, -maximum, std::numeric_limits<double>::denorm_min()}) {
        CAPTURE(value);
        CHECK(reduce(std::vector<double>(100'003, value)) == value);
    }
    std::vector<double> cancellation(100'002);
    for (std::size_t index = 0; index < cancellation.size(); ++index)
        cancellation[index] = index % 2 == 0 ? maximum : -maximum;
    CHECK(reduce(cancellation) == 0.0);
}
