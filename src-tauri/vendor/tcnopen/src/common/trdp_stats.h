/******************************************************************************/
/**
 * @file            trdp_stats.h
 *
 * @brief           Statistics for TRDP communication
 *
 * @details
 *
 * @note            Project: TCNOpen TRDP prototype stack
 *
 * @author          Bernd Loehr, NewTec GmbH
 *
 * @remarks This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. 
 *          If a copy of the MPL was not distributed with this file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *          Copyright Alstom SA or its subsidiaries and others, 2013-2023. All rights reserved.
 */
 /*
 * $Id: trdp_stats.h 2422 2023-10-23 15:57:53Z ahweiss $
 *
 */


#ifndef TRDP_STATS_H
#define TRDP_STATS_H

/*******************************************************************************
 * INCLUDES
 */

#include "trdp_if_light.h"
#include "trdp_private.h"
#include "vos_utils.h"

/*******************************************************************************
 * DEFINES
 */


/*******************************************************************************
 * TYPEDEFS
 */


/*******************************************************************************
 * GLOBAL FUNCTIONS
 */

void    trdp_initStats(TRDP_APP_SESSION_T appHandle);
void    trdp_pdPrepareStats (TRDP_APP_SESSION_T appHandle, PD_ELE_T *pPacket);


#endif
