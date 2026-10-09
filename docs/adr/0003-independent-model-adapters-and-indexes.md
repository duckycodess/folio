# Separate generation from multilingual retrieval

The user wants replaceable local models without accumulating specialized LLMs, and Filipino/Taglish is mandatory. Use one selected generative model and a compact multilingual embedding adapter, with task-specific measurements in Model Lab. Each embedding model revision has an isolated or rebuilt vector index; retaining a generative model's conversation must not mix incompatible embedding spaces.
