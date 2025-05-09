import { DenoDir } from "./deno_dir.ts";
import { NpmCache } from "./npm_cache.ts";

Deno.test("npm_cache.get_cache_location", async () => {
    console.log((new DenoDir()).root)
    const npmCache = await NpmCache.create({root: (new DenoDir()).root})
    console.log(npmCache.get_cache_location())
})