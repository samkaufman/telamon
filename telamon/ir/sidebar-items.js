initSidebarItems({"enum":[["AccessPattern",""],["BinOp","Represents binary arithmetic operators."],["DimMapScope","Indicates how dimensions can be mapped. The `L` type indicates how to lower mapped dimensions."],["Error","An error occuring while manipulating an ir instance."],["MemoryLevel","Indicates the slowest memory level where a variable may be stored."],["Operand","Represents an instruction operand."],["Operator","The operation performed by an instruction."],["StmtId","Provides a unique identifer for a basic block."],["Stride","A stride on a given dimensions."],["Type","Values and intructions types."],["TypeError","Errors that can be raised when creating an IR instance."],["UnaryOp","Arithmetic operators with a single operand."],["VarDef","Specifies how is a `Variable` defined."]],"mod":[["dim","Defines iteration dimensions properties."],["mem","A module for handling accesses to the device memory."],["op","Defines operators."],["prelude","Defines traits to import in the environment to use the IR."]],"struct":[["Body",""],["Counter","A wrapper used to count extra dimensions that will be needed in the future and allocates IDs for them. This is used when freezing in order to pre-allocate IDs for the various objects (internal memory block, instructions, dimensions, etc.) required for future lowering."],["DimId","Provides a unique identifier for iteration dimensions."],["DimMap","Represents a mapping between dimenions."],["DimMapping","Specifies that two dimensions should be mapped together."],["DimMappingId","Uniquely identifies a pair of mapped dimensions."],["Dimension","Represents an iteration dimension."],["Display","Helper struct for printing objects implementing [`IrDisplay`] with `format!` and `{}`."],["Function","Describes a function and the set of its possible implementation."],["IndVarId","Unique identifier for `InductionVar`"],["InductionVar","A multidimentional induction variable. No dimension should appear twice in dims."],["InstId","Uniquely identifies an instruction."],["Instruction","Represents an instruction."],["LogicalDim","A logic dimension composed of multiple `Dimension`s."],["LogicalDimId","Provides a unique identifier for logic dimensions."],["LoweredDimMap","A point-to-point communication lowered into a store and a load."],["LoweringMap",""],["NewObjs","Stores the objects created by a lowering."],["Parameter","Represents an argument of a function."],["PartialSize","A size whose exact value is not yet decided. The value of `size` is `product(size.factors())/product(size.divisors())`."],["Signature","Holds the signature of a function."],["Size","A fully specified size."],["VarId","Uniquely identifies variables."],["Variable","A variable produced by the code."]],"trait":[["IrDisplay","A trait for displaying objects which need a [`Function`] to be displayed."],["Statement","Represents a basic block in an Exhaust function."]]});