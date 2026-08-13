package com.umfl.api

import com.umfl.auth.CurrentManager
import com.umfl.manager.Manager
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RequestMapping
import org.springframework.web.bind.annotation.RestController

@RestController
@RequestMapping("/api/me")
class MeController {

    /** The signed-in manager — identity for the app's top bar. */
    @GetMapping
    fun me(@CurrentManager manager: Manager): ManagerDto = ManagerDto.from(manager)
}
