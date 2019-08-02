use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{atomic, Arc, Mutex};

use serde_json;
use structopt::StructOpt;

use telamon::device;
use telamon::explorer::{
    self,
    choice::{ActionEx as Action, Choice},
    config,
    eventlog::EventLog,
    mcts, Candidate,
};
use telamon::ir::IrDisplay;
use telamon::model::{bound, Bound};
use telamon::offline_analysis::tree::CandidateTree;
use telamon::search_space::SearchSpace;
use telamon_kernels::statistics::estimate_mean;

use telamon_cli::{Bench, KernelParam, Platform, ReplayPath};

/// Compute the bound for a given candidate.
#[derive(StructOpt)]
struct ComputeBound {
    #[structopt(long = "platform", default_value = "cuda")]
    platform: Platform,

    /// Kernel specification to use.
    #[structopt(short = "k", long = "kernel")]
    kernel: KernelParam,

    /// Path to a saved replay file to load before computing the bound.
    #[structopt(parse(from_os_str), short = "r", long = "replay")]
    replay: Option<ReplayPath>,
}

impl ComputeBound {
    fn run(&self, _args: &Opt) -> io::Result<()> {
        let builder = self.platform.to_builder();
        let mut context = builder.build_context();
        let (mut candidates, context) = context.kernel_build(&self.kernel);

        assert!(candidates.len() == 1);
        let mut candidate = candidates.swap_remove(0).space;

        // Apply replay if there is some
        if let Some(replay) = &self.replay {
            for action in &replay.load()? {
                candidate = action
                    .apply_to(candidate)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            }
        }

        let start = std::time::Instant::now();
        let bound = bound(&candidate, context);
        let duration = start.elapsed();
        println!("Bound is {:?} (computed in {:?})", bound, duration);

        Ok(())
    }
}

/// Compute bounds.csv
#[derive(StructOpt)]
struct Bounds {
    #[structopt(long = "platform", default_value = "cuda")]
    platform: Platform,

    #[structopt(long = "order")]
    order: Option<config::ChoiceOrdering>,

    #[structopt(long = "kernel")]
    kernel: KernelParam,

    #[structopt(long = "num-runs", default_value = "500")]
    num_runs: usize,
}

/// Ignore candidates with a too big bound in tests.
const CUT: f64 = 2e8f64;

impl Bounds {
    fn next_choice(&self, space: &SearchSpace) -> Option<Choice> {
        if let Some(order) = &self.order {
            explorer::choice::list(order, space).next()
        } else {
            explorer::choice::default_list(space).next()
        }
    }

    /// Descends along a path in the search tree and stores the bounds encountered on the way.
    fn random_descent(
        &self,
        candidates: &[Candidate],
        context: &dyn device::Context,
    ) -> Option<(Candidate, Vec<Bound>)> {
        let order = explorer::config::NewNodeOrder::Random;
        let mut candidates = Cow::Borrowed(candidates);
        let mut bounds = Vec::new();
        loop {
            let idx = if let Some(idx) = order.pick_candidate(&candidates, CUT) {
                idx
            } else {
                break None;
            };
            bounds.push(candidates[idx].bound.clone());
            let choice_opt = self.next_choice(&candidates[idx].space);
            if let Some(choice) = choice_opt {
                let new_nodes = candidates[idx]
                    .apply_choice(context, choice)
                    .into_iter()
                    .filter(|x| x.bound.value() < CUT)
                    .collect::<Vec<_>>();
                candidates = std::borrow::Cow::Owned(new_nodes);
            } else {
                break Some((
                    match candidates {
                        Cow::Borrowed(candidates) => candidates[idx].clone(),
                        Cow::Owned(mut candidates) => candidates.swap_remove(idx),
                    },
                    bounds,
                ));
            }
        }
    }

    fn test_bound<F>(
        &self,
        candidates: Vec<Candidate>,
        context: &dyn device::Context,
        body_fn: F,
    ) where
        F: Fn((f64, Vec<f64>)) + Sync,
    {
        let num_tested = atomic::AtomicUsize::new(0);
        let stabilizer = &context.stabilizer();
        context.async_eval(
            num_cpus::get(),
            device::EvalMode::TestBound,
            &|evaluator| loop {
                if num_tested.fetch_add(1, atomic::Ordering::SeqCst) >= self.num_runs {
                    if num_tested.fetch_sub(1, atomic::Ordering::SeqCst) > self.num_runs {
                        break;
                    }
                }

                if let Some((leaf, mut bounds)) =
                    self.random_descent(&candidates, context)
                {
                    evaluator.add_kernel(leaf, {
                        let body_fn = &body_fn;
                        move |leaf, kernel| {
                            let bound = leaf.bound.clone();
                            let runtime = stabilizer
                                .wrap(kernel)
                                .bound(Some(bound.value()))
                                .evaluate()
                                .unwrap();
                            bounds.push(bound);
                            body_fn((
                                runtime,
                                bounds.into_iter().map(|bound| bound.value()).collect(),
                            ))
                        }
                    });
                } else {
                    num_tested.fetch_sub(1, atomic::Ordering::SeqCst);
                }
            },
        );
    }

    fn run(&self, _args: &Opt) -> io::Result<()> {
        let builder = self.platform.to_builder();
        let mut context = builder.build_context();
        let (candidates, context) = context.kernel_build(&self.kernel);
        let stdout = std::io::stdout();
        self.test_bound(candidates, context, |(runtime, bounds)| {
            let mut handle = stdout.lock();
            write!(handle, "{},{}", self.kernel, runtime).unwrap();
            for bound in bounds {
                write!(handle, ",{}", bound).unwrap();
            }
            write!(handle, "\n").unwrap();
        });

        Ok(())
    }
}

#[derive(StructOpt)]
struct Codegen {
    #[structopt(parse(from_os_str))]
    replay: ReplayPath,

    /// Kernel specification to use.
    #[structopt(short = "k", long = "kernel")]
    kernel: KernelParam,

    #[structopt(long = "platform", short = "p", default_value = "cuda")]
    platform: Platform,
}

impl Codegen {
    fn run(&self, _args: &Opt) -> io::Result<()> {
        let builder = self.platform.to_builder();
        let mut context = builder.build_context();
        let (mut candidates, context) = context.kernel_build(&self.kernel);
        assert!(candidates.len() == 1);

        let mut candidate = candidates.swap_remove(0).space;
        for action in &self.replay.load()? {
            candidate = action
                .apply_to(candidate)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        }

        let code = telamon::codegen::Function::build(&candidate);
        context.device().print(&code, &mut std::io::stdout());

        Ok(())
    }
}

#[derive(StructOpt)]
struct Benchmark {
    #[structopt(parse(from_os_str))]
    replay: ReplayPath,

    /// Kernel specification to use.
    #[structopt(short = "k", long = "kernel")]
    kernel: KernelParam,

    #[structopt(long = "platform", short = "p", default_value = "cuda")]
    platform: Platform,

    #[structopt(long = "predicated")]
    predicated: bool,
}

impl Benchmark {
    fn run(&self, _args: &Opt) -> io::Result<()> {
        let builder = self.platform.to_builder();
        let mut context = builder.build_context();
        let (bundle, context) = context.kernel_bundle(&self.kernel);
        assert!(bundle.candidates.len() == 1);

        let mut candidate = bundle.candidates[0].space.clone();
        for action in &self.replay.load()? {
            candidate = action
                .apply_to(candidate)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        }

        let bound = bound(&candidate, context);

        let code = telamon::codegen::FunctionBuilder::new(&candidate)
            .predicated(self.predicated)
            .build();
        let runtimes = context.benchmark(&code, 40);
        (bundle.check_fn)(&Candidate::new(candidate, bound), context)
            .or_else(|err| Err(io::Error::new(io::ErrorKind::Other, err)))?;

        let reference = Bench::default()
            .warmup(4)
            .runs(40)
            .benchmark_fn(&bundle.reference_fn);
        println!(
            "runtime: {}, reference: {}",
            estimate_mean(runtimes, 0.95, "ns"),
            estimate_mean(reference, 0.95, "ns")
        );

        Ok(())
    }
}

/// Rebuild a specific list of actions from an event log.
///
/// This generates a .json replay file which can be used by the debugger, as well as by various
/// other utilities (eg the `compute_bound` subcommand).
#[derive(StructOpt)]
struct Rebuild {
    /// Path to the eventlog to rebuild from
    #[structopt(
        parse(from_os_str),
        short = "i",
        long = "input",
        default_value = "eventlog.tfrecord.gz"
    )]
    eventlog: PathBuf,

    /// Identifier of the candidate node to rebuild.  This corresponds to the ID indicated in
    /// `watch.log`.
    id: usize,
}

impl Rebuild {
    fn find_candidate(&self) -> io::Result<(mcts::NodeId, f64, Vec<Action>)> {
        let mut nevals = 0;
        let mut tree = CandidateTree::new();

        for record_bytes in EventLog::open(&self.eventlog)?.records() {
            match bincode::deserialize(&record_bytes?)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
            {
                mcts::Message::Node {
                    id,
                    parent,
                    mut children,
                    bound,
                    discovery_time,
                } => tree.extend(id, discovery_time, parent, bound, &mut children),
                mcts::Message::Trace { .. } => (),
                mcts::Message::Evaluation { id, value, .. } => {
                    if let Some(value) = value {
                        if nevals == self.id {
                            return Ok((id, value, tree.get_node(id).actions()));
                        }

                        nevals += 1;
                    }
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            "Unable to find candidate",
        ))
    }

    fn run(&self, _args: &Opt) -> io::Result<()> {
        let (id, score, actions) = self.find_candidate()?;

        println!("Found candidate {} (score: {})", id, score);
        println!("{}", serde_json::to_string_pretty(&actions)?);

        Ok(())
    }
}

/// Compute statistics on an eventlog
#[derive(StructOpt)]
struct Stats {
    /// Path to the eventlog to compute stats for
    #[structopt(
        parse(from_os_str),
        short = "i",
        long = "input",
        default_value = "eventlog.tfrecord.gz"
    )]
    eventlog: PathBuf,

    /// Maximum number of implementations to consider
    #[structopt(long = "limit")]
    limit: Option<usize>,
}

impl Stats {
    fn run(&self, _args: &Opt) -> io::Result<()> {
        let (mut nimpl, mut impld) = (0, 0u64);

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        enum Cause {
            Constraints,
            PerfModel,
            Backtrack,
        };

        impl From<mcts::CauseOfDeath> for Cause {
            fn from(cause: mcts::CauseOfDeath) -> Self {
                match cause {
                    mcts::CauseOfDeath::Constraints => Cause::Constraints,
                    mcts::CauseOfDeath::PerfModel { .. } => Cause::PerfModel,
                    mcts::CauseOfDeath::Backtrack => Cause::Backtrack,
                }
            }
        }

        let mut deadinfo = HashMap::new();

        let mut evalns = self.limit.map(Vec::with_capacity).unwrap_or_default();
        let mut tree = CandidateTree::new();

        for record_bytes in EventLog::open(&self.eventlog)?.records() {
            match bincode::deserialize(&record_bytes?)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
            {
                mcts::Message::Node {
                    id,
                    parent,
                    mut children,
                    bound,
                    discovery_time,
                } => tree.extend(id, discovery_time, parent, bound, &mut children),
                mcts::Message::Trace { events, .. } => {
                    let mut cause = None;
                    let mut len = 0;
                    let mut node = tree.get_root();
                    let mut has_size = false;

                    for event in &events {
                        match event.value {
                            mcts::Event::SelectNode(id) => {
                                node = tree.get_node(id);
                            }
                            mcts::Event::SelectChild(index, ..) => {
                                node = node
                                    .child(index.into())
                                    .unwrap_or_else(|| panic!("no child"));
                                match node.action().unwrap_or_else(|| panic!("no action"))
                                {
                                    Action::Action(
                                        telamon::search_space::Action::Size(..),
                                    ) => has_size = true,
                                    _ => (),
                                }
                                len += 1;
                            }
                            mcts::Event::KillChild(index, cause_) => {
                                let info = deadinfo
                                    .entry((Cause::from(cause_), has_size))
                                    .or_insert((0u64, 0u32));
                                info.0 += len + 1;
                                info.1 += 1;
                            }
                            mcts::Event::Kill(cause_) => {
                                assert!(cause.is_none());

                                cause = Some(Cause::from(cause_));
                            }
                            mcts::Event::Implementation => {
                                assert!(cause.is_none());

                                impld += len;
                                nimpl += 1;
                            }
                            mcts::Event::Expand => (),
                        }
                    }

                    if let Some(cause) = cause {
                        let info =
                            deadinfo.entry((cause, has_size)).or_insert((0u64, 0u32));
                        info.0 += len;
                        info.1 += 1;
                    }
                }
                mcts::Message::Evaluation { value, .. } => {
                    if let Some(value) = value {
                        evalns.push(value.log(10.));
                    }
                }
            }

            if self.limit.map(|limit| nimpl >= limit).unwrap_or(false) {
                break;
            }
        }

        let stats = stats::OnlineStats::from_slice(&evalns);
        println!(
            "Average log10 runtime: {:.2} (± {:.2})",
            stats.mean(),
            stats.stddev(),
        );

        println!(
            "Implementations: {} (avg depth: {})",
            nimpl,
            impld as f64 / nimpl as f64
        );

        let ((ddepth, ndead), (ddepth_size, ndead_size)) = deadinfo.iter().fold(
            ((0, 0), (0, 0)),
            |((ddepth, ndead), (ddepth_size, ndead_size)),
             ((_, has_size), (depth, num))| {
                if *has_size {
                    ((ddepth, ndead), (ddepth_size + depth, ndead_size + num))
                } else {
                    ((ddepth + depth, ndead + num), (ddepth_size, ndead_size))
                }
            },
        );

        println!(
            "Deadends: {} (avg depth: {})",
            ndead + ndead_size,
            (ddepth + ddepth_size) as f64 / (ndead + ndead_size) as f64
        );

        for ((cause, has_size), (cdepth, cnum)) in deadinfo.into_iter() {
            println!(
                "  - {:?} ({}): {} (avg depth: {})",
                cause,
                if has_size {
                    " (with size)"
                } else {
                    " (without size)"
                },
                cnum,
                cdepth as f64 / cnum as f64
            );
        }

        Ok(())
    }
}

#[derive(StructOpt)]
enum Command {
    #[structopt(name = "benchmark")]
    Benchmark(Benchmark),

    #[structopt(name = "codegen")]
    Codegen(Codegen),

    #[structopt(name = "rebuild")]
    Rebuild(Rebuild),

    #[structopt(name = "bounds")]
    Bounds(Bounds),

    #[structopt(name = "stats")]
    Stats(Stats),

    #[structopt(name = "bound")]
    Bound(ComputeBound),
}

#[derive(StructOpt)]
#[structopt(name = "telamon")]
struct Opt {
    #[structopt(subcommand)]
    command: Command,
}

fn main() {
    let args = Opt::from_args();
    env_logger::init();

    let result = match &args.command {
        Command::Benchmark(benchmark) => benchmark.run(&args),
        Command::Codegen(codegen) => codegen.run(&args),
        Command::Rebuild(rebuild) => rebuild.run(&args),
        Command::Bounds(bounds) => bounds.run(&args),
        Command::Stats(stats) => stats.run(&args),
        Command::Bound(bound) => bound.run(&args),
    };

    match result {
        Ok(()) => (),
        Err(err) => panic!("An error occured: {}", err),
    }
}
